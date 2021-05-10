# Singles in Your Area
Back in the dark ages before ad blockers, there existed a certain class of ad that promised to connect you with singles in your area. Of course, you'd ignore it, being the savvy consumer you are. "This is but a simple ad, it doesn't know where I live, therefore the premise is clearly false!" But wait... it has your city, maybe even your zip code! And so the seeds of doubt are sown.

## What it does
The application geolocates client's cities based on their IP address, then renders text containing that city over a template image before serving it to the client.

## Building

```shell
cargo build --color=always --workspace --all-targets --release
```

Alternatively, check the [latest releases](https://github.com/zkxs/singles-in-your-area/releases/latest) for prebuilt binaries.

## Running
- A file named `config.toml` must be present in the working directory. A documented example config is provided [here](examples/config.toml).
- Input images must be in the PNG format.
- A MaxMind GeoIP database must be present in the working directory, and must be named `GeoLite2-City.mmdb`

## Example Output
![example of a generated image](http://michaelripley.net:3035/ads/top_waifus.jpg)

## Why I made it
This project serves two purposes:
1. It's funny
2. It is an interesting demonstration of how fetching an asset from a remote server by necessity exposes your IP address.
