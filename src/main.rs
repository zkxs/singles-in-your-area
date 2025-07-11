// This file is part of singles-in-your-area and is licenced under the GNU AGPL v3.0.
// See LICENSE file for full text.
// Copyright © 2021-2025 Michael Ripley

use ab_glyph::FontVec;
use imageproc::drawing::{draw_text_mut, text_size};
use maxminddb::{Reader as MaxMindReader, geoip2};
use std::collections::HashMap;
use std::fs;
use std::io::Cursor;
use std::io::Write;
use std::net::{IpAddr, Ipv6Addr, SocketAddr};
use std::sync::LazyLock;
use tokio::sync::Semaphore;
use warp::Filter;
use warp::http::{HeaderMap, Response, StatusCode, header};

use crate::advert::*;

mod advert;

/// fallback fake location for when GeoIP lookup fails
const DEFAULT_CITY: &str = "your area";
const PORT: u16 = 3035;

type GeoIp = MaxMindReader<Vec<u8>>;
type Config = HashMap<String, Advert>;

static CONFIG: LazyLock<Config> = LazyLock::new(load_config);
static RENDER_SEMAPHORE: LazyLock<Semaphore> = LazyLock::new(|| Semaphore::new(2));
static PLAIN_TEXT_CONTENT_TYPE: &str = "text/plain; charset=utf-8";
static FONT: LazyLock<FontVec> = LazyLock::new(|| {
    FontVec::try_from_vec(Vec::from(include_bytes!("resources/DejaVuSans-Bold.ttf") as &[u8]))
        .expect("Unable to load font")
});
static GEOIP: LazyLock<GeoIp> = LazyLock::new(load_geoip_db);

/// load the config file and referenced images
fn load_config() -> Config {
    let config = fs::read_to_string("config.toml").expect("failed to open config.toml");
    let config: HashMap<String, AdvertDefinition> = toml::from_str(&config).expect("failed to deserialize config.toml");
    let config: Config = config.into_iter().map(|(path, ad)| (path, Advert::open(ad))).collect();
    config
}

/// open the GeoIP DB from disk
fn load_geoip_db() -> GeoIp {
    maxminddb::Reader::open_readfile("GeoLite2-City.mmdb").expect("failed to load geoip database")
}

#[tokio::main(flavor = "current_thread")]
async fn main() {
    println!(
        "[{}] Initializing {} {}",
        iso_string(),
        env!("CARGO_PKG_NAME"),
        env!("CARGO_PKG_VERSION")
    );

    println!("[{}] Done loading images", iso_string());

    // simple version endpoint at web root
    let root = warp::path::end()
        .and(warp::get())
        .map(|| format!("{} {}", env!("CARGO_PKG_NAME"), env!("CARGO_PKG_VERSION")));

    let ip = warp::path("ip")
        .and(warp::get())
        .and(warp::filters::addr::remote())
        .map(|addr: Option<SocketAddr>| {
            if let Some(addr) = addr {
                Response::builder()
                    .status(StatusCode::OK)
                    .header(header::CONTENT_TYPE, PLAIN_TEXT_CONTENT_TYPE)
                    .body(format!("{}", addr.ip()))
            } else {
                Response::builder()
                    .status(StatusCode::INTERNAL_SERVER_ERROR)
                    .header(header::CONTENT_TYPE, PLAIN_TEXT_CONTENT_TYPE)
                    .body("no ip address".to_string())
            }
        });

    let info = warp::path("info")
        .and(warp::get())
        .and(warp::filters::addr::remote())
        .and(warp::filters::header::headers_cloned())
        .map(info_handler);

    // the advert endpoint, hosted at /ads/<image_name>
    let adverts = warp::path!("ads" / String)
        .and(warp::get())
        .and(warp::filters::addr::remote())
        .then(fake_advert_handler);

    let routes = root.or(ip).or(info).or(adverts);

    println!("[{}] Starting web server on port {}...", iso_string(), PORT);
    warp::serve(routes).run((Ipv6Addr::UNSPECIFIED, PORT)).await;
}

/// current time as an ISO-8601 string
fn iso_string() -> String {
    format!("{:.3}", jiff::Timestamp::now())
}

/// handles a request to the /ad/<image_name> endpoint
async fn fake_advert_handler(image_name: String, socket_addr: Option<SocketAddr>) -> impl warp::Reply {
    match CONFIG.get(&image_name) {
        Some(advert) => {
            // attempt to generate the image
            let image = if let Some(socket_addr) = socket_addr {
                render_location_to_image(advert, get_city_from_ip(socket_addr.ip()))
                    .await
                    .map_err(|e| format!("Error encoding image: {e:?}"))
            } else {
                Err("no remote address".to_string())
            };

            match image {
                Ok(image) => {
                    // everything worked!
                    Response::builder()
                        .status(StatusCode::OK)
                        .header(header::CONTENT_TYPE, advert.output_format.mime_type())
                        .body(image)
                }
                Err(e) => {
                    // something went wrong with the image render
                    eprintln!("[{}] {}", iso_string(), e);
                    Response::builder()
                        .status(StatusCode::INTERNAL_SERVER_ERROR)
                        .header(header::CONTENT_TYPE, PLAIN_TEXT_CONTENT_TYPE)
                        .body(e.into())
                }
            }
        }
        None => {
            // someone requested an image_name that isn't in our config file
            eprintln!("[{}] 404: {}", iso_string(), image_name);
            Response::builder()
                .status(StatusCode::NOT_FOUND)
                .header(header::CONTENT_TYPE, PLAIN_TEXT_CONTENT_TYPE)
                .body("resource not found on server".into())
        }
    }
}

fn info_handler(socket_addr: Option<SocketAddr>, headers: HeaderMap) -> impl warp::Reply {
    let mut buf = Vec::new();
    let mut last_header_name = None;
    for (name, value) in headers.into_iter() {
        let name = if let Some(name) = name {
            last_header_name = Some(name);
            // we set this in the previous line: the unwrap cannot fail
            #[allow(clippy::unwrap_used)]
            last_header_name.as_ref().unwrap()
        } else {
            last_header_name.as_ref().expect("first header name was unset")
        };
        write!(buf, "{name}: ").expect("vec write failed");
        buf.extend_from_slice(value.as_bytes());
        buf.push(b'\n');
    }
    if let Some(socket_addr) = socket_addr {
        // write IP
        let ip = socket_addr.ip();
        writeln!(buf, "ip: {ip}").expect("vec write failed");

        // write ISP
        let isp = GEOIP.lookup::<geoip2::Isp>(ip).ok().flatten().and_then(|isp| isp.isp);
        if let Some(isp) = isp {
            writeln!(buf, "isp: {isp}").expect("vec write failed");
        }

        // write city
        let city = GEOIP
            .lookup::<geoip2::City>(ip)
            .ok()
            .flatten()
            .and_then(|city| city.city)
            .and_then(|city| city.names)
            .and_then(|names| names.first_key_value().map(|(_city, name)| *name));
        if let Some(city) = city {
            writeln!(buf, "city: {city}").expect("vec write failed");
        }

        // write country
        let country = GEOIP
            .lookup::<geoip2::Country>(ip)
            .ok()
            .flatten()
            .and_then(|country| country.country)
            .and_then(|country| country.names)
            .and_then(|names| names.first_key_value().map(|(_city, name)| *name));
        if let Some(country) = country {
            writeln!(buf, "country: {country}").expect("vec write failed");
        }
    }
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, PLAIN_TEXT_CONTENT_TYPE)
        .body(buf)
}

/// get an approximate city from an IP address, falling back to a default on failure
fn get_city_from_ip(addr: IpAddr) -> String {
    GEOIP
        .lookup::<geoip2::City>(addr)
        .ok()
        .flatten()
        .and_then(|city| city.city)
        .and_then(|city| city.names)
        .and_then(|names| names.first_key_value().map(|(_city, name)| (*name).to_owned()))
        .unwrap_or_else(|| DEFAULT_CITY.to_owned())
}

/// Render some custom text over an image, where that custom text contains a location (e.g. "singles near New York City").
///
/// To reduce potential DoS effect, the number of concurrent running render jobs is limited to 2.
async fn render_location_to_image(advert: &'static Advert, location: String) -> Result<Vec<u8>, String> {
    let render_permit = RENDER_SEMAPHORE
        .acquire()
        .await
        .expect("render semaphore has been closed!");
    // Even though renders are fast, they're not fast in the context of CPU clock speeds. Best to put it on a blocking task thread.
    let result = tokio::task::spawn_blocking(move || render_location_to_image_sync(advert, location))
        .await
        .unwrap_or_else(|_| Err("image render task failed to complete".to_string()));
    drop(render_permit); // be very explicit about where the permit is dropped
    result
}

/// actual synchronous implementation of [`render_location_to_image`]
fn render_location_to_image_sync(advert: &Advert, location: String) -> Result<Vec<u8>, String> {
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
    // this panic is relatively safe is it is not directly controlled by untrusted users: it relies on your config and their geoip lookup
    let text_width: i32 = text_width.try_into().expect("text width too large to fit in an i32");

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

    let image_result = if image.color().has_alpha() && !advert.output_format.has_alpha() {
        // handle the case where we need to throw away the alpha channel
        image
            .into_rgb8()
            .write_to(&mut Cursor::new(&mut buffer), advert.output_format.format())
    } else {
        image.write_to(&mut Cursor::new(&mut buffer), advert.output_format.format())
    };
    image_result.map_err(|e| format!("failed to encode output image: {e:?}"))?;
    Ok(buffer)
}
