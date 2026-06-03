phire::tl_file!("character");

use macroquad::texture::load_texture;
use phire::ext::SafeTexture;
use serde::{Deserialize, Serialize};
use anyhow::Result;

#[derive(Clone, Serialize, Deserialize)]
pub struct Character {
    pub id: String,
    pub name: String,
    pub intro: String,
    pub illust: String,
    pub illustrator: String,

    #[serde(default)]
    pub name_size: Option<f32>,
    #[serde(default)]
    pub baseline: bool,

    pub illu_adjust: (f32, f32, f32, f32),

    #[serde(skip)]
    pub illu: Option<SafeTexture>,
}

impl Character {
    pub async fn new() -> Result<Self> {
        let illu: SafeTexture = load_texture("/char/feilas.png").await?.into();
        // let illu = Texture::new(illu.width(), illu.height(), illu.data().to_vec());
        Ok(Self {
            id: "feilas".to_owned(),
            name: tl!("feilas-name").into_owned(),
            intro: tl!("feilas-intro").into_owned(),
            illust: "/char/feilas.png".to_owned(),
            illustrator: "零玖F".to_owned(),

            name_size: None,
            baseline: false,

            illu_adjust: (0.0, 0.2, 1.5, 1.5),

            illu: Some(illu.with_mipmap()),
        })
    }
}
