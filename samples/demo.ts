import { Camera } from "../index.ts";

const FPS = 60.0;

const camera = new Camera(1280, 720);
const pixels = new Uint8Array(1280 * 720 * 4);

let timer = 0;

camera.start();

setInterval(() => {
    for (let i = 0; i < 720; i++) {
        for (let j = 0; j < 1280; j++) {
            const off = i * 1280 + j;
            pixels[4 * off] = timer % 256;
            pixels[4 * off + 1] = 0;
            pixels[4 * off + 2] = i % 256;
            pixels[4 * off + 3] = 255;
        }
    }

    camera.send(pixels);
    timer++;
}, 1000.0 / FPS);
