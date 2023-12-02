extern crate byteorder;
extern crate defy;
pub extern crate image;
#[macro_use]
extern crate thiserror;

use byteorder::{LE, ReadBytesExt, WriteBytesExt};
use defy::{ContextualError, Contextualize};
use image::{Rgba, RgbaImage, ImageFormat, GenericImage};
use image::imageops::crop_imm;

use std::io::{self, Cursor, Read, Write};

#[derive(Debug, Error)]
pub enum Error {
  #[error("unsupported image format {0:?}")]
  UnsupportedImageFormat(ImageFormat),
  #[error(transparent)]
  Io(#[from] ContextualError<io::Error>),
  #[error(transparent)]
  Image(#[from] ContextualError<image::ImageError>)
}

const MAGIC_BYTES: [u8; 4] = *b"PZPK";
const END_OF_IMAGE: [u8; 4] = u32::to_le_bytes(0xDEADBEEF);

/// An entry within a page.
#[derive(Debug, Clone)]
pub struct Entry {
  pub name: String,
  pub x_pos: u32,
  pub y_pos: u32,
  pub width: u32,
  pub height: u32,
  pub x_offset: u32,
  pub y_offset: u32,
  pub total_width: u32,
  pub total_height: u32
}

impl Entry {
  pub fn get_image(&self, base: &RgbaImage) -> RgbaImage {
    let mut out = RgbaImage::from_pixel(self.total_width, self.total_height, Rgba([0; 4]));
    let view = crop_imm(base, self.x_pos, self.y_pos, self.width, self.height);
    out.copy_from(&*view, self.x_offset, self.y_offset).unwrap();
    out
  }

  fn read<R: Read>(mut reader: R) -> Result<Self, Error> {
    let name = read_string(&mut reader)
      .context("failed to read name for entry")?;
    let mut body = [0; 8];
    reader.read_u32_into::<LE>(&mut body)
      .context("failed to read body of entry")?;

    Ok(Entry {
      name,
      x_pos: body[0],
      y_pos: body[1],
      width: body[2],
      height: body[3],
      x_offset: body[4],
      y_offset: body[5],
      total_width: body[6],
      total_height: body[7]
    })
  }

  fn write<W: Write>(&self, mut writer: W) -> Result<(), Error> {
    write_string(&mut writer, &self.name)
      .context("failed to write name for entry")?;
    for num in [
      self.x_pos, self.y_pos, self.width, self.height,
      self.x_offset, self.y_offset, self.total_width, self.total_height
    ] {
      writer.write_u32::<LE>(num)
        .context("failed to write body of entry")?;
    };

    Ok(())
  }
}

/// A page within a pack file.
/// Pages are like texture atlases, with each [`Entry`] defining the sub-images.
#[derive(Debug, Clone)]
pub struct Page {
  pub name: String,
  pub mask: i32,
  pub entries: Vec<Entry>,
  pub image: RgbaImage
}

impl Page {
  pub const DEFAULT_MASK: i32 = 1;

  pub fn new(name: String, entries: Vec<Entry>, image: RgbaImage) -> Self {
    Page { name, mask: Self::DEFAULT_MASK, entries, image }
  }

  pub fn get_entry_image(&self, index: usize) -> Option<RgbaImage> {
    self.entries.get(index).map(|entry| entry.get_image(&self.image))
  }

  fn read_v1<R: Read>(mut reader: R) -> Result<Self, Error> {
    let name = read_string(&mut reader)
      .context("failed to read name for page")?;
    let entries_len = reader.read_u32::<LE>()
      .context("failed to read entries_len for page")?;
    let mask = reader.read_i32::<LE>()
      .context("failed to read mask for page")?;
    let entries = (0..entries_len)
      .map(|_| Entry::read(&mut reader))
      .collect::<Result<Vec<Entry>, Error>>()?;

    let image_buf = read_until_pattern(&mut reader, &END_OF_IMAGE)
      .context("failed to read image contents for page")?;
    let image = image::load_from_memory(&image_buf)
      .context("failed to decode image contents for page")?
      .into_rgba8();

    Ok(Page {
      name,
      mask,
      entries,
      image
    })
  }

  fn read_v2<R: Read>(mut reader: R) -> Result<Self, Error> {
    let name = read_string(&mut reader)
      .context("failed to read name for page")?;
    let entries_len = reader.read_u32::<LE>()
      .context("failed to read entries_len for page")?;
    let mask = reader.read_i32::<LE>()
      .context("failed to read mask for page")?;
    let entries = (0..entries_len)
      .map(|_| Entry::read(&mut reader))
      .collect::<Result<Vec<Entry>, Error>>()?;

    let image_buf = read_buffer(&mut reader)
      .context("failed to read image contents for page")?;
    let image = image::load_from_memory(&image_buf)
      .context("failed to decode image contents for page")?
      .into_rgba8();

    Ok(Page {
      name,
      mask,
      entries,
      image
    })
  }

  fn write_v1<W: Write>(&self, mut writer: W) -> Result<(), Error> {
    write_string(&mut writer, &self.name)
      .context("failed to write name for page")?;
    writer.write_u32::<LE>(self.entries.len() as u32)
      .context("failed to write entries_len for page")?;
    writer.write_i32::<LE>(self.mask)
      .context("failed to write mask for page")?;
    for entry in self.entries.iter() {
      entry.write(&mut writer)?;
    };

    write_png(&mut writer, &self.image)
      .context("failed to encode image contents for page")?;
    writer.write_all(&END_OF_IMAGE)
      .context("failed to write image terminator")?;

    Ok(())
  }

  fn write_v2<W: Write>(&self, mut writer: W) -> Result<(), Error> {
    write_string(&mut writer, &self.name)
      .context("failed to write name for page")?;
    writer.write_u32::<LE>(self.entries.len() as u32)
      .context("failed to write entries_len for page")?;
    writer.write_i32::<LE>(self.mask)
      .context("failed to write mask for page")?;
    for entry in self.entries.iter() {
      entry.write(&mut writer)?;
    };

    let mut buf = Cursor::new(Vec::new());
    write_png(&mut buf, &self.image)
      .context("failed to encode image contents for page")?;
    write_buffer(&mut writer, buf.get_ref())
      .context("failed to write image contents for page")?;

    Ok(())
  }
}

/// The full contents of a pack file.
#[derive(Debug, Clone)]
pub struct Pack {
  /// On `V2` pack files, an extra number is provided before the pages count (similar to [`Page`]'s `mask`).
  ///
  /// I'm not sure what it is used for, so I am naming it `mask`.
  pub mask: i32,
  pub pages: Vec<Page>
}

impl Pack {
  pub const DEFAULT_MASK: i32 = 1;

  pub fn new(pages: Vec<Page>) -> Self {
    Pack { mask: Self::DEFAULT_MASK, pages }
  }

  pub fn read<R: Read>(mut reader: R) -> Result<Self, Error> {
    let mut magic_bytes = [0; 4];
    reader.read_exact(&mut magic_bytes)
      .context("failed to read pack")?;
    if magic_bytes == MAGIC_BYTES {
      Pack::read_v2(reader)
    } else {
      let reader = Cursor::new(magic_bytes).chain(reader);
      Pack::read_v1(reader)
    }
  }

  fn read_v1<R: Read>(mut reader: R) -> Result<Self, Error> {
    let pages_len = reader.read_u32::<LE>()
      .context("failed to read pages_len for pack")?;
    (0..pages_len).map(|_| Page::read_v1(&mut reader))
      .collect::<Result<Vec<Page>, Error>>()
      .map(|pages| Pack { mask: Self::DEFAULT_MASK, pages })
  }

  fn read_v2<R: Read>(mut reader: R) -> Result<Self, Error> {
    let mask = reader.read_i32::<LE>()
      .context("failed to read pack")?;
    let pages_len = reader.read_u32::<LE>()
      .context("failed to read pages_len for pack")?;
    (0..pages_len).map(|_| Page::read_v2(&mut reader))
      .collect::<Result<Vec<Page>, Error>>()
      .map(|pages| Pack { mask, pages })
  }

  #[inline]
  pub fn write<W: Write>(&self, writer: W) -> Result<(), Error> {
    self.write_v2(writer)
  }

  #[inline]
  pub fn write_with<W: Write>(&self, writer: W, version: FormatVersion) -> Result<(), Error> {
    match version {
      FormatVersion::V1 => self.write_v1(writer),
      FormatVersion::V2 => self.write_v2(writer)
    }
  }

  fn write_v1<W: Write>(&self, mut writer: W) -> Result<(), Error> {
    writer.write_u32::<LE>(self.pages.len() as u32)
      .context("failed to write pages_len for pack")?;
    for page in self.pages.iter() {
      page.write_v1(&mut writer)?;
    };

    writer.flush()
      .context("failed to flush writer")?;

    Ok(())
  }

  fn write_v2<W: Write>(&self, mut writer: W) -> Result<(), Error> {
    writer.write_all(&MAGIC_BYTES)
      .context("failed to write pack")?;
    writer.write_i32::<LE>(self.mask)
      .context("failed to write pack")?;
    writer.write_u32::<LE>(self.pages.len() as u32)
      .context("failed to write pages_len for pack")?;
    for page in self.pages.iter() {
      page.write_v2(&mut writer)?;
    };

    writer.flush()
      .context("failed to flush writer")?;

    Ok(())
  }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum FormatVersion {
  /// "V1" is a placeholder name for the format described in <https://pzwiki.net/wiki/File_formats>.
  V1,
  /// "V2" is a placeholder name for a similar format, that I have not been able to find described anywhere.
  ///
  /// Other than a few differences, it is exactly the same as "V1":
  /// - The file is prefixed with four bytes, `PZPK`.
  /// - An `int32` of an unknown purpose comes before the `int32` marking the number of pages.
  /// - Images do not end with `0xDEADBEEF`, and instead have an `int32`/`uint32` prepended describing length in bytes.
  ///
  /// Packs will be saved with "V2" by default.
  V2
}

impl Default for FormatVersion {
  #[inline]
  fn default() -> Self {
    FormatVersion::V2
  }
}

fn write_string<W: Write>(mut writer: W, s: &str) -> io::Result<()> {
  writer.write_u32::<LE>(s.len() as u32)?;
  writer.write_all(s.as_bytes())?;
  Ok(())
}

fn write_buffer<W: Write>(mut writer: W, b: &[u8]) -> io::Result<()> {
  writer.write_u32::<LE>(b.len() as u32)?;
  writer.write_all(b)?;
  Ok(())
}

pub fn write_png<W: Write>(writer: W, image: &RgbaImage) -> image::ImageResult<()> {
  use image::{ColorType, ImageEncoder};
  use image::codecs::png::{CompressionType, FilterType, PngEncoder};
  PngEncoder::new_with_quality(writer, CompressionType::Best, FilterType::default())
    .write_image(image.as_raw(), image.width(), image.height(), ColorType::Rgba8)?;
  Ok(())
}

pub fn read_png<R: Read>(reader: R) -> image::ImageResult<RgbaImage> {
  use image::DynamicImage;
  use image::codecs::png::PngDecoder;
  let image = DynamicImage::from_decoder(PngDecoder::new(reader)?)?;
  Ok(image.into_rgba8())
}

fn read_string<R: Read>(mut reader: R) -> io::Result<String> {
  let len = reader.read_u32::<LE>()?;
  let mut buf = String::with_capacity(len as usize);
  reader.take(len as u64).read_to_string(&mut buf)?;
  Ok(buf)
}

fn read_buffer<R: Read>(mut reader: R) -> io::Result<Vec<u8>> {
  let len = reader.read_u32::<LE>()?;
  let mut buf = Vec::with_capacity(len as usize);
  reader.take(len as u64).read_to_end(&mut buf)?;
  Ok(buf)
}

fn read_until_pattern<R: Read>(mut reader: R, pat: &[u8]) -> io::Result<Vec<u8>> {
  let mut buf = Vec::new();
  let len = loop {
    if let Some(stripped) = buf.strip_suffix(pat) {
      break stripped.len();
    } else {
      buf.push(reader.read_u8()?);
    };
  };

  buf.truncate(len);
  Ok(buf)
}
