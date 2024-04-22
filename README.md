# Point Cloud

A point cloud renderer and converter.

## Converter

In order to render a point cloud you have to convert it with the converter.
Use `--help` for an explanation on how to use it.

`cargo run -p point-converter --release -- --help`

The converter will generate a `metadata.json` which needs to be opened from the renderer.
In the web version the directory with the `metadata.json` needs to be selected.

If a `metadata.json` file already exists in the output directory, the converter will merge all new points into the
found `metadata.json`.

## How to run

Install `cargo-make`:

`cargo install --force cargo-make`

Run a task from `Makefile.toml` with:

### Local

Start local:
`cargo make release`

### Web

Build for Web:
`cargo make web`

Then serve the `index.html` with for example `miniserve`.