import { rmSync } from "fs";

rmSync("build", {
    recursive: true,
    force: true,
});
