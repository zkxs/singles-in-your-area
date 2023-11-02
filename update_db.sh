#!/bin/bash
rm -rf GeoLite2-City.mmdb.old > /dev/null
mv GeoLite2-City.mmdb GeoLite2-City.mmdb.old
MAXMIND_LICENSE="$(< MAXMIND_LICENSE.txt)"
wget -O "GeoLite2-City.mmdb.tar.gz" "https://download.maxmind.com/app/geoip_download?edition_id=GeoLite2-City&license_key=$MAXMIND_LICENSE&suffix=tar.gz"
tar xzf "GeoLite2-City.mmdb.tar.gz"
mv "$(find . -name "GeoLite2-City_*" -type d | head -n1)/GeoLite2-City.mmdb" .
find . -name "GeoLite2-City_*" -type d -exec rm -rf "{}" \;
rm "GeoLite2-City.mmdb.tar.gz"

