extern crate byteorder;
extern crate defy;
extern crate image;
extern crate pz_pack;
extern crate steamlocate;
extern crate thiserror;
extern crate walkdir;

use defy::Contextualize;
use pz_pack::Pack;
use steamlocate::SteamDir;
use walkdir::WalkDir;

use std::fs::File;



// Reads every .pack file from a local install of Project Zomboid, and attempts to parse them.

#[test]
fn test_official_texture_packs() -> Result<(), Box<dyn std::error::Error>> {
  let steam_dir = SteamDir::locate().expect("failed to locate Steam installation");
  let (project_zomboid, library) = steam_dir.find_app(108600)
    .context("failed to locate a Project Zomboid Steam installation")?
    .ok_or_else(|| "failed to locate a Project Zomboid Steam installation")?;

  let project_zomboid_install_dir = library.resolve_app_dir(&project_zomboid);
  let project_zomboid_media_dir = project_zomboid_install_dir.join("media");
  println!("Project Zomboid media dir: {}", project_zomboid_media_dir.display());

  for result in WalkDir::new(&project_zomboid_media_dir) {
    let entry = result.context_path("failed to traverse directory", &project_zomboid_media_dir)?;
    if entry.file_type().is_file() && entry.path().extension().is_some_and(|ext| ext.eq_ignore_ascii_case("pack")) {
      println!("Reading texture pack: {}", entry.path().display());
      let file = File::open(entry.path()).context_path("failed to open file", entry.path())?;

      Pack::read(file)?;
    };
  };

  Ok(())
}
