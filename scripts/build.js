import { execSync } from "child_process";

execSync("napi build --release --platform --esm --output-dir build", {
    stdio: "inherit",
});
