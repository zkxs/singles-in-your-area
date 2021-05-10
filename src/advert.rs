use image::{DynamicImage, ImageFormat, Rgba};
use image::io::Reader as ImageReader;
use rusttype::Scale;
use serde::Deserialize;

#[derive(Deserialize)]
pub struct AdvertDefinition {
    pub image: String,
    pub image_width: u32,
    pub image_height: u32,
    pub frames: u32,
    /// true to center, false to left-align
    pub text_align: Align,
    /// left OR center of text
    pub text_x: u32,
    /// top of text
    pub text_y: u32,
    pub text_color: [u8; 4],
    pub text_scale: f32,
    pub text_case: Case,
    pub output_format: ImageOutput,
    pub text_prefix: String,
}

pub struct Advert {
    pub image: DynamicImage,
    pub image_width: u32,
    pub image_height: u32,
    pub frames: u32,
    /// true to center, false to left-align
    pub text_align: Align,
    /// left OR center of text
    pub text_x: u32,
    /// top of text
    pub text_y: u32,
    pub text_color: Rgba<u8>,
    pub text_scale: Scale,
    pub text_case: Case,
    pub output_format: ImageOutput,
    pub text_prefix: String,
}

impl Advert {
    pub fn open(definition: AdvertDefinition) -> Advert {
        let mut reader = ImageReader::open(definition.image.clone())
            .expect(format!("failed to open image: {}", definition.image).as_str());
        reader.set_format(ImageFormat::Png);
        let image = reader.decode().expect("failed to decode image");

        Advert {
            image,
            image_width: definition.image_width,
            image_height: definition.image_height,
            frames: definition.frames,
            text_align: definition.text_align,
            text_x: definition.text_x,
            text_y: definition.text_y,
            text_color: Rgba(definition.text_color),
            text_scale: Scale {
                x: definition.text_scale,
                y: definition.text_scale,
            },
            text_case: definition.text_case,
            output_format: definition.output_format,
            text_prefix: definition.text_prefix,
        }
    }
}

#[derive(Deserialize)]
pub enum ImageOutput {
    Jpeg,
    Png,
}

impl ImageOutput {
    pub fn format(&self) -> ImageFormat {
        match &self {
            ImageOutput::Jpeg => ImageFormat::Jpeg,
            ImageOutput::Png => ImageFormat::Png,
        }
    }

    pub fn mime_type(&self) -> &'static str {
        match &self {
            ImageOutput::Jpeg => "image/jpeg",
            ImageOutput::Png => "image/png",
        }
    }
}

#[derive(Deserialize)]
pub enum Align {
    Left,
    Center,
}

#[derive(Deserialize)]
pub enum Case {
    Default,
    Upper,
}
