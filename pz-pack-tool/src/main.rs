#![warn(
  absolute_paths_not_starting_with_crate,
  redundant_imports,
  redundant_lifetimes,
  future_incompatible,
  deprecated_in_future,
  missing_copy_implementations,
  missing_debug_implementations,
  unnameable_types,
  unreachable_pub
)]

extern crate anyhow;
extern crate clap;
extern crate fs_err as fs;
extern crate glam;
extern crate indexmap;
extern crate pz_pack;
extern crate serde;
extern crate toml;



use anyhow::{Error, Context, bail};
use clap::Parser;
use glam::UVec2;
use pz_pack::{Pack, Page, Entry, FormatVersion};
use pz_pack::image::RgbaImage;
use serde::{Deserialize, Serialize};
use serde::de::DeserializeOwned;
use indexmap::map::IndexMap;

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::io::{BufReader, BufWriter};



fn main() {
  Args::try_parse()
    .and_then(|args| {
      args.run().map_err(|error| {
        use clap::error::{Error, ErrorKind};
        Error::raw(ErrorKind::Io, format_args!("{error:?}"))
      })
    })
    .unwrap_or_else(|error| {
      error.print().expect("error");
    });
}

/// Packs/unpacks Project Zomboid's texture pack files.
#[derive(Debug, Parser)]
#[command(version, author)]
enum Args {
  /// Packs the given directory into a .pack file.
  #[command(name = "pack")]
  Pack {
    /// The path to the source directory to pack.
    #[arg(id = "in")]
    in_path: PathBuf,
    /// The path to the destination for the produced pack file to be placed.
    #[arg(id = "out")]
    out_path: PathBuf,
    /// Restricts the packed pages to only those listed.
    #[arg(long)]
    just: Vec<String>
  },
  /// Unpacks the given .pack file into a directory.
  #[command(name = "unpack")]
  Unpack {
    /// The path to the pack file to unpack.
    #[arg(id = "in")]
    in_path: PathBuf,
    /// The path to the destination for the produced directory to be placed.
    #[arg(id = "out")]
    out_path: PathBuf,
    /// Restricts the unpacked pages to only those listed.
    #[arg(long)]
    just: Vec<String>
  }
}

impl Args {
  fn run(self) -> Result<(), Error> {
    match self {
      Args::Pack { in_path, out_path, just } => pack(in_path, out_path, just),
      Args::Unpack { in_path, out_path, just } => unpack(in_path, out_path, just)
    }
  }
}

fn unpack(in_path: PathBuf, out_path: PathBuf, just: Vec<String>) -> Result<(), Error> {
  let just = just.into_iter().collect::<HashSet<String>>();
  let pack_file = fs::File::open(&in_path)
    .context("failed to open pack file")?;
  let pack = Pack::read(BufReader::new(pack_file))
    .context("failed to read pack file")?;

  let (pack_definition, pages) = PackDefinition::from_pack(pack);

  fs::create_dir_all(&out_path).context("failed to create dir")?;
  save_toml(out_path.join("pack.toml"), &pack_definition)?;

  for (page_definition, image) in pages {
    if just.is_empty() || just.contains(&page_definition.name) {
      let out_path_page = out_path.join(normalize_name(&page_definition.name));
      fs::create_dir_all(&out_path_page).context("failed to create dir")?;

      save_toml(out_path_page.join("page.toml"), &page_definition)?;
      save_png(out_path_page.join("page.png"), &image)?;
    };
  };

  Ok(())
}

fn pack(in_path: PathBuf, out_path: PathBuf, just: Vec<String>) -> Result<(), Error> {
  let just = just.into_iter().collect::<HashSet<String>>();
  let pack_definition = load_toml::<PackDefinition>(in_path.join("pack.toml"))?;
  let mut pages = Vec::new();
  for result in fs::read_dir(&in_path).context("failed to read dir")? {
    let entry = result.context("failed to read dir entry")?;
    let file_type = entry.file_type().context("failed to read dir entry file type")?;
    if !file_type.is_dir() { continue };

    let in_path_page = in_path.join(entry.file_name());

    let page_definition = load_toml::<PageDefinition>(in_path_page.join("page.toml"))?;
    let image = load_png(in_path_page.join("page.png"))?;

    if just.is_empty() || just.contains(&page_definition.name) {
      pages.push((page_definition, image));
    };
  };

  let pack = pack_definition.into_pack(pages)?;

  let writer = fs::File::create(&out_path)
    .context("failed to create pack")?;
  pack.write(BufWriter::new(writer))
    .context("failed to write pack")?;

  Ok(())
}

fn load_png(path: impl Into<PathBuf>) -> Result<RgbaImage, Error> {
  let image = pz_pack::read_png(fs::File::open(path)?)?;
  Ok(image)
}

fn save_png(path: impl Into<PathBuf>, image: &RgbaImage) -> Result<(), Error> {
  pz_pack::write_png(fs::File::create(path)?, image)?;
  Ok(())
}

fn load_toml<T: DeserializeOwned>(path: impl AsRef<Path>) -> Result<T, Error> {
  let value = toml::from_str(&fs::read_to_string(path)?)?;
  Ok(value)
}

fn save_toml<T: Serialize>(path: impl AsRef<Path>, value: &T) -> Result<(), Error> {
  fs::write(path, toml::ser::to_string_pretty(value)?)?;
  Ok(())
}



#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Deserialize, Serialize)]
enum PackDefinitionVersion {
  V0, V1
}

impl PackDefinitionVersion {
  fn into_format_version(self) -> FormatVersion {
    match self {
      PackDefinitionVersion::V0 => FormatVersion::V0,
      PackDefinitionVersion::V1 => FormatVersion::V1
    }
  }

  fn from_format_version(format_version: FormatVersion) -> Self {
    match format_version {
      FormatVersion::V0 => PackDefinitionVersion::V0,
      FormatVersion::V1 => PackDefinitionVersion::V1
    }
  }
}

impl Default for PackDefinitionVersion {
  fn default() -> Self {
    PackDefinitionVersion::V1
  }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
struct PackDefinition {
  #[serde(default)]
  version: PackDefinitionVersion
}

impl PackDefinition {
  fn from_pack(pack: Pack) -> (Self, Vec<(PageDefinition, RgbaImage)>) {
    let version = PackDefinitionVersion::from_format_version(pack.version);
    (PackDefinition { version }, pack.pages.into_iter().map(PageDefinition::from_page).collect())
  }

  fn into_pack(self, pages: impl IntoIterator<Item = (PageDefinition, RgbaImage)>) -> Result<Pack, Error> {
    let version = self.version.into_format_version();
    let pages = pages.into_iter()
      .map(|(page_definition, image)| page_definition.into_page(image))
      .collect::<Result<Vec<Page>, Error>>()?;
    Ok(Pack::new(version, pages))
  }
}

#[derive(Debug, Deserialize, Serialize)]
struct PageDefinition {
  name: String,
  #[serde(default = "default_mask")]
  mask: bool,
  entries: IndexMap<String, EntryDefinition>
}

impl PageDefinition {
  fn from_page(page: Page) -> (Self, RgbaImage) {
    let name = page.name;
    let mask = page.mask;
    let entries = page.entries.into_iter()
      .map(|entry| EntryDefinition::from_entry(entry))
      .collect();
    (PageDefinition { name, mask, entries }, page.image)
  }

  fn into_page(self, image: RgbaImage) -> Result<Page, Error> {
    let entries = self.entries.into_iter()
      .map(|(name, entry_definition)| entry_definition.into_entry(name, &image))
      .collect::<Result<Vec<Entry>, Error>>()?;
    Ok(Page::with_mask(self.name, self.mask, entries, image))
  }
}

#[derive(Debug, Deserialize, Serialize)]
struct EntryDefinition {
  pos: UVec2,
  size: UVec2,
  #[serde(flatten)]
  frame: Option<EntryFrameDefinition>
}

impl EntryDefinition {
  fn from_entry(entry: Entry) -> (String, Self) {
    let name = entry.name;
    let pos = UVec2::new(entry.x_pos, entry.y_pos);
    let size = UVec2::new(entry.width, entry.height);
    let frame_offset = UVec2::new(entry.x_offset, entry.y_offset);
    let frame_size = UVec2::new(entry.total_width, entry.total_height);
    let frame = (frame_offset != UVec2::ZERO && frame_size != size)
      .then_some(EntryFrameDefinition { offset: frame_offset, size: frame_size });
    (name, EntryDefinition { pos, size, frame })
  }

  fn into_entry(self, name: String, image: &RgbaImage) -> Result<Entry, Error> {
    let image_size = UVec2::from(image.dimensions());
    let frame = self.frame.unwrap_or(EntryFrameDefinition { offset: UVec2::ZERO, size: self.size });

    if self.size == UVec2::ZERO || image_size.cmplt(self.pos + self.size).any() {
      bail!("sub-image too small ({1} > {2}) for entry {0}", name, self.size, image_size);
    };

    if frame.size == UVec2::ZERO || self.size.cmpgt(frame.offset + frame.size).any() {
      bail!("frame too small ({1} < {2}) for entry {0}", name, frame.size, self.size);
    };

    Ok(Entry {
      name,
      x_pos: self.pos.x,
      y_pos: self.pos.y,
      width: self.size.x,
      height: self.size.y,
      x_offset: frame.offset.x,
      y_offset: frame.offset.y,
      total_width: frame.size.x,
      total_height: frame.size.y
    })
  }
}

#[derive(Debug, Deserialize, Serialize)]
struct EntryFrameDefinition {
  #[serde(rename = "frame_offset")]
  offset: UVec2,
  #[serde(rename = "frame_size")]
  size: UVec2
}

fn default_mask() -> bool {
  true
}



fn normalize_name(name: &str) -> String {
  name.chars()
    .filter_map(|ch| match ch {
      ch if ch.is_ascii_control() => None,
      '<' | '>' | ':' | '"' | '|' | '?' | '*' => None,
      '/' | '\\' | '.' => Some('_'),
      ch => Some(ch)
    })
    .collect::<String>()
}
