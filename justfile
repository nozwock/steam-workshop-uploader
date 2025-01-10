set unstable
set quiet
set script-interpreter := ["nu"]

default:
  just --list


# Bundle binary with steamworks
[script]
bundle bin_name:
    let arch = $"(cargo rustc -- -vV | grep 'host:' | cut -d ' ' -f 2)"
    mut bin_name = {{ quote(bin_name) }}
    let steamworks_lib_path = $"(cargo build --release --bin $bin_name --message-format json | jq -r 'select(.reason == "build-script-executed" and (.package_id | contains("steamworks-sys"))) | .out_dir')"

    if "{{ os() }}" == "windows" {
        $bin_name += ".exe"
    }

    mkdir dist/tmp
    cp $"target/release/($bin_name)" dist/tmp/
    cp ($"($steamworks_lib_path)/*" | into glob) dist/tmp/

    cd dist/tmp
    let filename = $"($bin_name)-($arch).zip"
    7z a $filename *

    mv $filename ../
    cd ../
    rm -r tmp/
