#[macro_use]
extern crate lazy_static;

use std::collections::HashMap;
use std::fs;
use std::io::Cursor;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::sync::Arc;

use ab_glyph::FontVec;
use chrono::{SecondsFormat, Utc};
use imageproc::drawing::{draw_text_mut, text_size};
use maxminddb::{geoip2, Reader as MaxMindReader};
use warp::http::{Response, StatusCode};
use warp::Filter;

use crate::advert::*;

mod advert;

/// fallback fake location for when GeoIP lookup fails
const DEFAULT_CITY: &str = "your area";
const PORT: u16 = 3035;

type GeoIp = MaxMindReader<Vec<u8>>;
type Config = HashMap<String, Advert>;

lazy_static! {
    static ref FONT: FontVec =
        FontVec::try_from_vec(Vec::from(include_bytes!("resources/DejaVuSans-Bold.ttf") as &[u8])).unwrap();
    static ref GEOIP: GeoIp = load_geoip_db();
}

fn load_geoip_db() -> GeoIp {
    maxminddb::Reader::open_readfile("GeoLite2-City.mmdb").expect("failed to load geoip database")
}

#[tokio::main]
async fn main() {
    println!(
        "[{}] Initializing {} {}",
        iso_string(),
        env!("CARGO_PKG_NAME"),
        env!("CARGO_PKG_VERSION")
    );

    // load the config file and referenced images
    let config = fs::read_to_string("config.toml").expect("failed to open config.toml");
    let config: HashMap<String, AdvertDefinition> = toml::from_str(&config).expect("failed to deserialize config.toml");
    let config: Config = config.into_iter().map(|(path, ad)| (path, Advert::open(ad))).collect();
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

    let routes = info.or(adverts);

    println!("[{}] Starting web server on port {}...", iso_string(), PORT);
    tokio::join!(
        warp::serve(routes.clone()).run((Ipv4Addr::UNSPECIFIED, PORT)),
        warp::serve(routes).run((Ipv6Addr::UNSPECIFIED, PORT)),
    );
}

/// helper function making it easier to pass state warp filters
fn with_state<T: Clone + Send>(state: T) -> impl Filter<Extract = (T,), Error = std::convert::Infallible> + Clone {
    warp::any().map(move || state.clone())
}

/// current time as an ISO-8601 string
fn iso_string() -> String {
    Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true)
}

/// handles a request to the /ad/<image_name> endpoint
async fn fake_advert_handler(
    image_name: String,
    config: Arc<Config>,
    socket_addr: Option<SocketAddr>,
) -> Result<impl warp::Reply, warp::Rejection> {
    match (*config).get(&image_name) {
        Some(advert) => {
            // attempt to generate the image
            let image = socket_addr
                .ok_or_else(|| "no remote address".to_string())
                .and_then(|socket_addr| {
                    render_location_to_image(advert, get_city_from_ip(socket_addr.ip()))
                        .map_err(|e| format!("Error encoding PNG: {:?}", e))
                });

            match image {
                Ok(image) => {
                    // everything worked!
                    Ok(Response::builder()
                        .status(StatusCode::OK)
                        .header("Content-Type", advert.output_format.mime_type())
                        .body(image))
                }
                Err(e) => {
                    // something went wrong with the the image render
                    eprintln!("[{}] {}", iso_string(), e);
                    Ok(Response::builder()
                        .status(StatusCode::INTERNAL_SERVER_ERROR)
                        .header("Content-Type", "text/plain")
                        .body(e.into()))
                }
            }
        }
        None => {
            // someone requested an image_name that isn't in our config file
            eprintln!("[{}] 404: {}", iso_string(), image_name);
            Ok(Response::builder()
                .status(StatusCode::NOT_FOUND)
                .header("Content-Type", "text/plain")
                .body("resource not found on server".into()))
        }
    }
}

/// get an approximate city from an IP address, falling back to a default on failure
fn get_city_from_ip(addr: IpAddr) -> String {
    (*GEOIP)
        .lookup(addr)
        .ok()
        .and_then(|city: geoip2::City| city.city)
        .and_then(|city| city.names)
        .and_then(|names| names.iter().next().map(|(_k, v)| v.to_owned()))
        .map(|v| v.to_owned())
        .unwrap_or_else(|| DEFAULT_CITY.to_owned())
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
    let location: String = match advert.text_case {
        Case::Default => location,
        Case::Upper => location.to_uppercase(),
    };

    // figure out how wide the text is
    let text: String = format!("{}{}", advert.text_prefix, location);
    let (text_width, _text_height): (u32, _) = text_size(text_scale, &*FONT, &text);
    let text_width: i32 = text_width.try_into().unwrap();

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
    image
        .write_to(&mut Cursor::new(&mut buffer), advert.output_format.format())
        .map_err(|e| format!("failed to encode output image: {:?}", e))?;
    Ok(buffer)
}
