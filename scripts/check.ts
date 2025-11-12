import { execSync } from "child_process";

execSync('prettier --check "**/*.{js,ts,json}"', { stdio: "inherit" });
execSync('eslint . --ext ".ts,.js"', { stdio: "inherit" });
