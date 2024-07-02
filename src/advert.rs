use ab_glyph::PxScale;
use const_format::formatcp;
use image::{DynamicImage, ImageFormat, Rgba};
use image::io::Reader as ImageReader;
use serde::Deserialize;

/// simple struct that maps to config file entries
#[derive(Deserialize)]
pub struct AdvertDefinition {
    pub image: String,
    pub image_width: u32,
    pub image_height: u32,
    /// number of frames, used for animation sprite sheets (currently only vertical stacking is supported)
    pub frames: u32,
    pub text_align: Align,
    /// left OR center of text, depending on text_align
    pub text_x: u32,
    /// top of text
    pub text_y: u32,
    /// RGBA values
    pub text_color: [u8; 4],
    pub text_scale: f32,
    pub text_case: Case,
    pub output_format: ImageOutput,
    /// prefix for GeoIP location
    pub text_prefix: String,
}

/// fancier struct that we get after a bit of config post-processing
pub struct Advert {
    pub image: DynamicImage,
    pub image_width: i32,
    pub image_height: i32,
    /// number of frames, used for animation sprite sheets (currently only vertical stacking is supported)
    pub frames: i32,
    pub text_align: Align,
    /// left OR center of text, depending on text_align
    pub text_x: i32,
    /// top of text
    pub text_y: i32,
    pub text_color: Rgba<u8>,
    pub text_scale: PxScale,
    pub text_case: Case,
    pub output_format: ImageOutput,
    /// prefix for GeoIP location
    pub text_prefix: String,
}

impl Advert {
    /// load an Advert from its definition. Notably this loads a PNG image from disk into memory
    pub fn open(definition: AdvertDefinition) -> Advert {
        let mut reader = ImageReader::open(definition.image.clone())
            .unwrap_or_else(|e| panic!("failed to open image \"{}\": {:?}", definition.image, e));
        reader.set_format(ImageFormat::Png);
        let image = reader.decode().expect("failed to decode image");

        Advert {
            image,
            image_width: i32::try_from(definition.image_width).expect(formatcp!("image_width must be less than {}", i32::MAX)),
            image_height: i32::try_from(definition.image_height).expect(formatcp!("image_height must be less than {}", i32::MAX)),
            frames: i32::try_from(definition.frames).expect(formatcp!("frames must be less than {}", i32::MAX)),
            text_align: definition.text_align,
            text_x: i32::try_from(definition.text_x).expect(formatcp!("text_x must be less than {}", i32::MAX)),
            text_y: i32::try_from(definition.text_y).expect(formatcp!("text_y must be less than {}", i32::MAX)),
            text_color: Rgba(definition.text_color),
            text_scale: PxScale {
                x: definition.text_scale,
                y: definition.text_scale,
            },
            text_case: definition.text_case,
            output_format: definition.output_format,
            text_prefix: definition.text_prefix,
        }
    }
}

/// all the different output formats we support
#[derive(Deserialize)]
pub enum ImageOutput {
    Jpeg,
    Png,
}

impl ImageOutput {
    /// image "format": used by our image processing library
    pub fn format(&self) -> ImageFormat {
        match &self {
            ImageOutput::Jpeg => ImageFormat::Jpeg,
            ImageOutput::Png => ImageFormat::Png,
        }
    }

    /// image mime type: used by our web server
    pub fn mime_type(&self) -> &'static str {
        match &self {
            ImageOutput::Jpeg => "image/jpeg",
            ImageOutput::Png => "image/png",
        }
    }
}

/// supported text alignment options
#[derive(Deserialize)]
pub enum Align {
    Left,
    Center,
}

/// supported text case options
#[derive(Deserialize)]
pub enum Case {
    /// the exact string the GeoIP lookup gives us
    Default,
    /// forced uppercase
    Upper,
}
