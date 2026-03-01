use rust_embed::RustEmbed;

#[derive(RustEmbed, Clone)]
#[folder = "dist/"]
pub struct FrontendDist;
