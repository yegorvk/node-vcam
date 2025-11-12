import { execSync } from "child_process";

execSync('prettier --write "**/*.{js,ts,json}"', { stdio: "inherit" });
execSync("cargo fmt", { stdio: "inherit" });
