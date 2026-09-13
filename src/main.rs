mod decisions;
mod disliked;
mod index_cache;
mod input;
mod progress;
mod provider;
mod sweep;

use anyhow::Result;

fn main() -> Result<()> {
    #[cfg(not(target_os = "macos"))]
    {
        anyhow::bail!("track-sweep currently requires macOS (Apple Music / Music.app).");
    }

    #[cfg(target_os = "macos")]
    {
        let provider = provider::apple_music::AppleMusicProvider::new();
        sweep::run(&provider)?;
    }

    Ok(())
}
