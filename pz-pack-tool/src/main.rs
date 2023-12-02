extern crate clap;
extern crate defy;
extern crate glam;
extern crate pz_pack;
extern crate serde;
#[macro_use]
extern crate thiserror;
extern crate toml;



use clap::Parser;
use defy::{ContextualError, Contextualize};
use glam::UVec2;
use pz_pack::{Pack, Page, Entry};
use pz_pack::image::RgbaImage;
use serde::{Deserialize, Serialize};

use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;
use std::io::{self, BufReader, BufWriter};
use std::ffi::OsStr;
use std::fs::File;



#[derive(Debug, Parser)]
#[command(name = "pz-pack")]
#[command(author = "ScottyThePilot")]
#[command(version = "0.1.0")]
#[command(about = "Packs/unpacks Project Zomboid texturepack files", long_about = None)]
enum Cli {
  /// Packs a directory of .png and .toml files into a .pack file.
  Pack {
    /// The path to the source directory to pack.
    #[arg(id = "in")]
    in_path: PathBuf,
    /// The path to the destination for the produced pack file to be placed.
    #[arg(id = "out")]
    out_path: PathBuf
  },
  /// Unpacks a given .pack file into a directory.
  Unpack {
    /// The path to the pack file to unpack.
    #[arg(id = "in")]
    in_path: PathBuf,
    /// The path to the destination for the produced directory to be placed.
    #[arg(id = "out")]
    out_path: PathBuf
  },
  /// Unpacks a given page from a given .pack file into a directory.
  UnpackPage {
    /// The path to the pack file to unpack.
    #[arg(id = "in")]
    in_path: PathBuf,
    /// The path to the destination for the produced directory to be placed.
    #[arg(id = "out")]
    out_path: PathBuf,
    /// The page who's entries should be extracted.
    #[arg(id = "page")]
    page_name: String
  }
}

fn main() {
  let result = match Cli::parse() {
    Cli::Pack { in_path, out_path } => pack(in_path, out_path),
    Cli::Unpack { in_path, out_path } => unpack(in_path, out_path),
    Cli::UnpackPage { in_path, out_path, page_name } => unpack_page(in_path, out_path, page_name)
  };

  if let Err(error) = result {
    eprintln!("{error}");
  };
}

#[derive(Debug, Error)]
enum Error {
  #[error("sub-image too small ({1} > {2}) for entry {0}")]
  SubImageTooBig(String, UVec2, UVec2),
  #[error("frame too small ({1} < {2}) for entry {0}")]
  FrameTooSmall(String, UVec2, UVec2),
  #[error("page {0:?} not found in pack")]
  PageDoesNotExist(String),
  #[error(transparent)]
  Io(#[from] ContextualError<io::Error>),
  #[error(transparent)]
  Image(#[from] ContextualError<pz_pack::image::ImageError>),
  #[error(transparent)]
  TomlDe(#[from] ContextualError<toml::de::Error>),
  #[error(transparent)]
  TomlSer(#[from] ContextualError<toml::ser::Error>),
  #[error(transparent)]
  PackError(#[from] ContextualError<pz_pack::Error>)
}

fn unpack_page(in_path: PathBuf, out_path: PathBuf, page_name: String) -> Result<(), Error> {
  let pack_file = File::open(&in_path).map(BufReader::new)
    .context_path("failed to open pack file", &in_path)?;
  let pack = Pack::read(pack_file)
    .context_path("failed to read pack file", &in_path)?;

  let mut dir_created = false;
  let page = pack.pages.iter()
    .find(|page| page.name == page_name)
    .ok_or(Error::PageDoesNotExist(page_name))?;
  for entry in page.entries.iter() {
    if !dir_created {
      std::fs::create_dir_all(&out_path)
        .context_path("failed to create dir", &out_path)?;
      dir_created = true;
    };

    let image = entry.get_image(&page.image);

    let png_path = out_path.join(&format!("{}.png", entry.name));
    let png_writer = File::create(&png_path).map(BufWriter::new)
      .context_path("failed to create image", &png_path)?;
    pz_pack::write_png(png_writer, &image)
      .context_path("failed to write image", &png_path)?;
  };

  Ok(())
}

fn unpack(in_path: PathBuf, out_path: PathBuf) -> Result<(), Error> {
  let pack_file = File::open(&in_path).map(BufReader::new)
    .context_path("failed to open pack file", &in_path)?;
  let pack = Pack::read(pack_file)
    .context_path("failed to read pack file", &in_path)?;

  let mut dir_created = false;
  for page in pack.pages {
    if !dir_created {
      std::fs::create_dir_all(&out_path)
        .context_path("failed to create dir", &out_path)?;
      dir_created = true;
    };

    let (name, page_config, image) = PageConfig::from_page(page);
    let toml_path = out_path.join(&format!("{name}.toml"));
    let png_path = out_path.join(&format!("{name}.png"));

    let toml_buf = toml::to_string_pretty(&page_config)
      .context(format!("failed to serialize page {name}"))?;
    std::fs::write(&toml_path, &toml_buf)
      .context_path("failed to write page", &toml_path)?;

    let png_writer = File::create(&png_path).map(BufWriter::new)
      .context_path("failed to create image", &png_path)?;
    pz_pack::write_png(png_writer, &image)
      .context_path("failed to write image", &png_path)?;
  };

  Ok(())
}

fn pack(in_path: PathBuf, out_path: PathBuf) -> Result<(), Error> {
  let mut toml_files = HashMap::new();
  let mut png_files = HashMap::new();
  for result in std::fs::read_dir(&in_path).context_path("failed to read dir", &in_path)? {
    let entry = result.context_path("failed to read dir entry", &in_path)?;
    let file_type = entry.file_type().context_path("failed to read dir entry file type", &in_path)?;
    if !file_type.is_file() { continue };

    let path = entry.path();
    let stem = path.file_stem().and_then(OsStr::to_str);
    let ext = path.extension().and_then(OsStr::to_str);
    match Option::zip(stem, ext) {
      Some((stem, ext)) if ext.eq_ignore_ascii_case("toml") => {
        toml_files.insert(stem.to_owned(), path);
      },
      Some((stem, ext)) if ext.eq_ignore_ascii_case("png") => {
        png_files.insert(stem.to_owned(), path);
      },
      _ => continue
    };
  };

  let mut pages = Vec::new();
  for (name, toml_path) in toml_files.iter() {
    let Some(png_path) = png_files.get(name) else { continue };

    let toml_buf = std::fs::read_to_string(toml_path)
      .context_path("failed to read file", toml_path)?;
    let page_config = toml::from_str::<PageConfig>(&toml_buf)
      .context_path("failed to read page config", toml_path)?;

    let png_reader = File::open(png_path).map(BufReader::new)
      .context_path("failed to open image file", png_path)?;
    let image = pz_pack::read_png(png_reader)
      .context_path("failed to read image file", png_path)?;

    pages.push(page_config.into_page(name.clone(), image)?);
  };

  let pack = Pack::new(pages);
  let writer = File::create(&out_path).map(BufWriter::new)
    .context_path("failed to create pack", &out_path)?;
  pack.write(writer).context_path("failed to write pack", &out_path)?;

  Ok(())
}



#[derive(Debug, Deserialize, Serialize)]
#[serde(transparent)]
struct PageConfig {
  entries: BTreeMap<String, EntryConfig>
}

impl PageConfig {
  fn from_page(page: Page) -> (String, Self, RgbaImage) {
    let entries = page.entries.into_iter()
      .map(|entry| EntryConfig::from_entry(entry))
      .collect();
    (page.name, PageConfig { entries }, page.image)
  }

  fn into_page(self, name: String, image: RgbaImage) -> Result<Page, Error> {
    let entries = self.entries.into_iter()
      .map(|(name, entry_config)| entry_config.into_entry(name, &image))
      .collect::<Result<Vec<Entry>, Error>>()?;
    Ok(Page::new(name, entries, image))
  }
}

#[derive(Debug, Deserialize, Serialize)]
struct EntryConfig {
  pos: UVec2,
  size: UVec2,
  #[serde(flatten)]
  frame: Option<EntryConfigFrame>
}

impl EntryConfig {
  fn from_entry(entry: Entry) -> (String, Self) {
    let name = entry.name;
    let pos = UVec2::new(entry.x_pos, entry.y_pos);
    let size = UVec2::new(entry.width, entry.height);
    let frame_offset = UVec2::new(entry.x_offset, entry.y_offset);
    let frame_size = UVec2::new(entry.total_width, entry.total_height);
    let frame = (frame_offset != pos && frame_size != size)
      .then_some(EntryConfigFrame { offset: frame_offset, size: frame_size });
    (name, EntryConfig { pos, size, frame })
  }

  fn into_entry(self, name: String, image: &RgbaImage) -> Result<Entry, Error> {
    let image_size = UVec2::from(image.dimensions());
    let frame = self.frame.unwrap_or(EntryConfigFrame { offset: UVec2::ZERO, size: self.size });

    if self.size == UVec2::ZERO || image_size.cmplt(self.pos + self.size).any() {
      return Err(Error::SubImageTooBig(name, self.size, image_size));
    };

    if frame.size == UVec2::ZERO || self.size.cmpgt(frame.offset + frame.size).any() {
      return Err(Error::FrameTooSmall(name, frame.size, self.size))
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
struct EntryConfigFrame {
  #[serde(rename = "frame_offset")]
  offset: UVec2,
  #[serde(rename = "frame_size")]
  size: UVec2
}
