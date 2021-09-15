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
use rusttype::{Font, point, Scale};
use warp::Filter;
use warp::http::{Response, StatusCode};

use crate::advert::*;

mod advert;

/// fallback fake location for when GeoIP lookup fails
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

    // load the config file and referenced images
    let config = fs::read_to_string("config.toml").expect("failed to open config.toml");
    let config: HashMap<String, AdvertDefinition> = toml::from_str(&config).expect("failed to deserialize config.toml");
    let config: Config = config.into_iter()
        .map(|(path, ad)| (path, Advert::open(ad)))
        .collect();
    let config = Arc::new(config);

    println!("[{}] Done loading images", iso_string());

    // simple version endpoint at web root
    let info = warp::path::end()
        .and(warp::get())
        .map(|| format!("{} {}", env!("CARGO_PKG_NAME"), env!("CARGO_PKG_VERSION")));

    // the advert endpoint, hosted at /ads/<image_name>
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

/// helper function making it easier to pass state warp filters
fn with_state<T: Clone + Send>(state: T) -> impl Filter<Extract=(T, ), Error=std::convert::Infallible> + Clone {
    warp::any().map(move || state.clone())
}

/// current time as an ISO-8601 string
fn iso_string() -> String {
    Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true)
}

/// handles a request to the /ad/<image_name> endpoint
async fn fake_advert_handler(image_name: String, config: Arc<Config>, socket_addr: Option<SocketAddr>) -> Result<impl warp::Reply, warp::Rejection> {
    match (*config).get(&image_name) {
        Some(advert) => {

            // attempt to generate the image
            let image = socket_addr
                .ok_or("no remote address".to_string())
                .and_then(|socket_addr| {
                    render_location_to_image(advert, get_city_from_ip(socket_addr.ip()))
                        .map_err(|e| format!("Error encoding PNG: {:?}", e))
                });

            match image {
                Ok(image) => {
                    // everything worked!
                    Ok(
                        Response::builder()
                            .status(StatusCode::OK)
                            .header("Content-Type", advert.output_format.mime_type())
                            .body(image)
                    )
                }
                Err(e) => {
                    // something went wrong with the the image render
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
            // someone requested an image_name that isn't in our config file
            eprintln!("[{}] 404: {}", iso_string(), image_name);
            Ok(
                Response::builder()
                    .status(StatusCode::NOT_FOUND)
                    .header("Content-Type", "text/plain")
                    .body("resource not found on server".into())
            )
        }
    }
}

/// get an approximate city from an IP address, falling back to a default on failure
fn get_city_from_ip(addr: IpAddr) -> String {
    (*GEOIP).lookup(addr).ok()
        .and_then(|city: geoip2::City| city.city)
        .and_then(|city| city.names)
        .and_then(|names| names.iter().next().map(|(_k, v)| v.clone()))
        .map(|v| v.to_owned())
        .unwrap_or(DEFAULT_CITY.to_owned())
}

/// render some custom text over an image, where that custom text contains a location (e.g. "singles near New York City")
fn render_location_to_image(advert: &Advert, location: String) -> Result<Vec<u8>, String> {
    // we need a fresh copy of the image to render to
    let mut image = advert.image.clone();

    // grab a bunch of fields out of the config just for ease of use later
    let image_width = advert.image_width;
    let image_height = advert.image_height;
    let text_x = advert.text_x;
    let text_y = advert.text_y;
    let text_scale = advert.text_scale;

    // handle the desired text case
    let location = match advert.text_case {
        Case::Default => location,
        Case::Upper => location.to_uppercase()
    };

    // figure out how wide the text is
    let text = format!("{}{}", advert.text_prefix, location);
    let (width, _text_height) = text_size(text_scale, &*FONT, &text);
    let text_width = u32::try_from(width).map_err(|e| format!("error calculating text width: {:?}", e))?;

    // calculate x coordinate if we're centering the text
    let x = match advert.text_align {
        Align::Left => text_x,
        Align::Center => text_x.checked_sub(text_width / 2).unwrap_or(0),
    };

    // some special logging for the edge case where the text renders off the side of the image
    if x + text_width > image_width {
        let overflow = (x + text_width) - image_width;
        println!("[{}] hit, overflowed by {}px", iso_string(), overflow);
    } else {
        println!("[{}] hit", iso_string());
    }

    // render the text
    for frame in 0..advert.frames {
        let y = text_y + frame * image_height;
        draw_text_mut(&mut image, advert.text_color, x, y, text_scale, &*FONT, &text);
    }

    // encode the image
    let mut buffer: Vec<u8> = Vec::new();
    image.write_to(&mut buffer, advert.output_format.format())
        .map_err(|e| format!("failed to encode output image: {:?}", e))?;
    Ok(buffer)
}

/// Get the width and height of the given text, rendered with the given font and scale.
/// Note that this function *does not* support newlines, you must do this manually.
fn text_size(scale: Scale, font: &Font, text: &str) -> (i32, i32) {
    let v_metrics = font.v_metrics(scale);
    let (mut w, mut h) = (0, 0);
    for g in font.layout(text, scale, point(0.0, v_metrics.ascent)) {
        if let Some(bb) = g.pixel_bounding_box() {
            w = max(w, bb.max.x);
            h = max(h, bb.max.y);
        }
    }
    (w, h)
}
