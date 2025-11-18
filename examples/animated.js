import { Backend, ImageFormat, VirtualCamera } from "../index.js";

const FPS = 60;

const camera = new VirtualCamera(Backend.UnityCapture);
const image = new Uint8Array(1280 * 720 * 4);

let timer = 0;
camera.start();

setInterval(() => {
    for (let i = 0; i < 720; i++) {
        for (let j = 0; j < 1280; j++) {
            const off = i * 1280 + j;
            image[4 * off] = timer % 256;
            image[4 * off + 1] = 0;
            image[4 * off + 2] = i % 256;
            image[4 * off + 3] = 255;
        }
    }

    camera.send(1280, 720, ImageFormat.Rgba8Linear, image);
    timer++;
}, 1000.0 / FPS);
