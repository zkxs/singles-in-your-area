#[macro_use]
extern crate lazy_static;

use std::cmp::max;
use std::collections::HashMap;
use std::convert::TryFrom;
use std::fs;
use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;

use chrono::{SecondsFormat, Utc};
use imageproc::drawing::draw_text_mut;
use maxminddb::{geoip2, Reader as MaxMindReader};
use rusttype::{Font, point, PositionedGlyph, Rect, Scale};
use warp::Filter;
use warp::http::{Response, StatusCode};

use crate::advert::*;

mod advert;

const DEFAULT_CITY: &str = "your area";

type GeoIp = MaxMindReader<Vec<u8>>;
type Config = HashMap<String, Advert>;

lazy_static! {
    static ref FONT: Font<'static> = Font::try_from_vec(Vec::from(include_bytes!("resources/DejaVuSans-Bold.ttf") as &[u8])).unwrap();
    static ref GEOIP: GeoIp = load_geoip_db();
}

fn load_geoip_db() -> GeoIp {
    maxminddb::Reader::open_readfile("GeoLite2-City.mmdb")
        .expect("failed to load geoip database")
}

#[tokio::main]
async fn main() {
    println!("[{}] Initializing {} {}", iso_string(), env!("CARGO_PKG_NAME"), env!("CARGO_PKG_VERSION"));

    let server_address: SocketAddr = ([0, 0, 0, 0], 3035).into();

    let config = fs::read_to_string("config.toml").expect("failed to open config.toml");
    let config: HashMap<String, AdvertDefinition> = toml::from_str(&config).expect("failed to deserialize config.toml");
    let config: Config = config.into_iter()
        .map(|(path, ad)| (path, Advert::open(ad)))
        .collect();
    let config = Arc::new(config);

    println!("[{}] Done loading images", iso_string());

    let info = warp::path::end()
        .and(warp::get())
        .map(|| format!("{} {}", env!("CARGO_PKG_NAME"), env!("CARGO_PKG_VERSION")));

    let adverts = warp::path!("ads" / String)
        .and(warp::get())
        .and(with_state(config.clone()))
        .and(warp::filters::addr::remote())
        .and_then(fake_advert_handler);

    let routes = info
        .or(adverts);

    println!("[{}] Starting web server on {}...", iso_string(), server_address);
    warp::serve(routes)
        .run(server_address)
        .await;
}

fn with_state<T: Clone + Send>(state: T) -> impl Filter<Extract=(T, ), Error=std::convert::Infallible> + Clone {
    warp::any().map(move || state.clone())
}

fn iso_string() -> String {
    Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true)
}

async fn fake_advert_handler(path: String, config: Arc<Config>, socket_addr: Option<SocketAddr>) -> Result<impl warp::Reply, warp::Rejection> {
    match (*config).get(&path) {
        Some(advert) => {
            let mime_type = advert.output_format.mime_type();

            let image = socket_addr
                .ok_or("no remote address".to_string())
                .and_then(|socket_addr| ip_to_image(advert, socket_addr.ip()).map_err(|e| format!("Error encoding PNG: {:?}", e)));

            match image {
                Ok(image) => {
                    Ok(
                        Response::builder()
                            .status(StatusCode::OK)
                            .header("Content-Type", mime_type)
                            .body(image)
                    )
                }
                Err(e) => {
                    eprintln!("[{}] {}", iso_string(), e);
                    Ok(
                        Response::builder()
                            .status(StatusCode::INTERNAL_SERVER_ERROR)
                            .header("Content-Type", "text/plain")
                            .body(e.into())
                    )
                }
            }
        }
        None => {
            eprintln!("[{}] 404: {}", iso_string(), path);
            Ok(
                Response::builder()
                    .status(StatusCode::NOT_FOUND)
                    .header("Content-Type", "text/plain")
                    .body("resource not found on server".into())
            )
        }
    }
}

fn ip_to_location(addr: IpAddr) -> String {
    (*GEOIP).lookup(addr).ok()
        .and_then(|city: geoip2::City| city.city)
        .and_then(|city| city.names)
        .and_then(|names| names.iter().next().map(|(_k, v)| v.clone()))
        .map(|v| v.to_owned())
        .unwrap_or(DEFAULT_CITY.to_owned())
}

fn ip_to_image(advert: &Advert, addr: IpAddr) -> Result<Vec<u8>, String> {
    let mut image = advert.image.clone();
    let image_width = advert.image_width;
    let image_height = advert.image_height;
    let frames = advert.frames;
    let text_align = &advert.text_align;
    let text_case = &advert.text_case;
    let text_x = advert.text_x;
    let text_y = advert.text_y;
    let text_color = advert.text_color;
    let text_scale = advert.text_scale;
    let text_prefix = advert.text_prefix.as_str();
    let output_format = advert.output_format.format();

    let location = ip_to_location(addr);

    let location = match text_case {
        Case::Default => location,
        Case::Upper => location.to_uppercase()
    };

    let text = format!("{}{}", text_prefix, location);

    let (width, _text_height) = text_size(text_scale, &*FONT, &text);
    let text_width = u32::try_from(width).map_err(|e| format!("error calculating text width: {:?}", e))?;

    let x = match text_align {
        Align::Left => text_x,
        Align::Center => text_x.checked_sub(text_width / 2).unwrap_or(0),
    };

    if x + text_width > image_width {
        let overflow = (x + text_width) - image_width;
        println!("[{}] hit, overflowed by {}px", iso_string(), overflow);
    } else {
        println!("[{}] hit", iso_string());
    }

    for frame in 0..frames {
        let y = text_y + frame * image_height;
        draw_text_mut(&mut image, text_color, x, y, text_scale, &*FONT, &text);
    }

    let mut buffer: Vec<u8> = Vec::new();
    image.write_to(&mut buffer, output_format).expect("failed to encode output image");
    Ok(buffer)
}

fn layout_glyphs(
    scale: Scale,
    font: &Font,
    text: &str,
    mut f: impl FnMut(PositionedGlyph, Rect<i32>),
) -> (i32, i32) {
    let v_metrics = font.v_metrics(scale);

    let (mut w, mut h) = (0, 0);

    for g in font.layout(text, scale, point(0.0, v_metrics.ascent)) {
        if let Some(bb) = g.pixel_bounding_box() {
            w = max(w, bb.max.x);
            h = max(h, bb.max.y);
            f(g, bb);
        }
    }

    (w, h)
}

/// Get the width and height of the given text, rendered with the given font and scale. Note that this function *does not* support newlines, you must do this manually.
pub fn text_size(scale: Scale, font: &Font, text: &str) -> (i32, i32) {
    layout_glyphs(scale, font, text, |_, _| {})
}
