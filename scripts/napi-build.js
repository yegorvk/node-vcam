import { NapiCli } from "@napi-rs/cli";

const isRelease = process.argv.slice(2).some((arg) => {
    const key = arg.split("=", 2)[0];
    return key.toLowerCase() == "--release";
});

const outputDir = isRelease ? "build/release" : "build/debug";

new NapiCli().build({
    release: isRelease,
    esm: true,
    platform: true,
    outputDir: outputDir,
});
