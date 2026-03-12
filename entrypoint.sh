#!/bin/sh
set -e

DATA_DIR="${DATA_DIR:-/data}"

download_pbf() {
    mkdir -p "$DATA_DIR/pbf"
    for url in $PBF_URLS; do
        filename=$(basename "$url")
        if [ ! -f "$DATA_DIR/pbf/$filename" ]; then
            echo "Downloading $url..."
            curl -fSL -o "$DATA_DIR/pbf/$filename" "$url"
        else
            echo "Already downloaded: $filename"
        fi
    done
}

build_index() {
    files=""
    for f in "$DATA_DIR"/pbf/*.osm.pbf; do
        [ -f "$f" ] && files="$files $f"
    done
    if [ -z "$files" ]; then
        echo "Error: no PBF files found in $DATA_DIR/pbf/"
        exit 1
    fi
    mkdir -p "$DATA_DIR/index"
    echo "Building index..."
    build-index "$DATA_DIR/index" $files
    echo "Index built."
}

serve() {
    args="$DATA_DIR/index"
    if [ -n "$DOMAIN" ]; then
        args="$args --domain $DOMAIN"
        if [ -n "$CACHE_DIR" ]; then
            args="$args --cache $CACHE_DIR"
        fi
    else
        args="$args ${BIND_ADDR:-0.0.0.0:3000}"
    fi
    echo "Starting server..."
    exec query-server $args
}

case "${1:-auto}" in
    build)
        download_pbf
        build_index
        ;;
    serve)
        serve
        ;;
    auto)
        download_pbf
        build_index
        serve
        ;;
    *)
        exec "$@"
        ;;
esac
