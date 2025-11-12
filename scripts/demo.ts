import { execSync } from "child_process";

execSync("node scripts/build.ts", { stdio: "inherit" });
execSync("node samples/demo.ts", { stdio: "inherit" });
