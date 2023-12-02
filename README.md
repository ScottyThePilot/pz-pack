# PZ-Pack
Library/tool for managing Project Zomboid's .pack files

## Usage
Pack files consist of pages, with each page having an image file attached to it,
that functions like a texture atlas. Pages consist of entries, which are like
sub-images of the page's texture atlas.

### Unpacking
```bash
pz-pack-tool unpack ./InputFile.pack ./OutputDir
```

This takes an input .pack file (`./InputFile.pack`) and unpacks all of the
contained PNG files (and their page/entry info) into a directory (`./OutputDir`).

```bash
pz-pack-tool unpack-page ./InputFile.pack ./OutputDir PageName
```

This takes an input .pack file (`./InputFile.pack`) and unpacks all of the
entries of a specified page into a directory (`./OutputDir`).

### Packing
```bash
pz-pack-tool pack ./InputDir ./OutputFile.pack
```

This takes an input directory (`./InputDir`) and packs all of the PNG and TOML
files from it into a .pack file (`./OutputFile.pack`).

## TOML File Format
TOML files for pages will have the following format:

```toml
# An entry, this one's name is `Moodle_Bkg_Good_1`
[Moodle_Bkg_Good_1]
# The X and Y coordinate of the top-left corner of the sub-texture
pos = [33, 33]
# The width and height of the sub-texture
size = [31, 31]
# The number of pixels of transparent padding to the left and top of the texture
# Optional, defaults to [0, 0]
frame_offset = [0, 1]
# The final intended width and height of the sub-texture (padding will be added)
# Optional, must be greater than or equal to `size` from above, defaults to `size`
frame_size = [32, 32]
```
