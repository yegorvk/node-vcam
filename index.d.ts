/**
 * Represents a virtual camera device.
 */
export declare class VirtualCamera {
    /**
     * Creates a new VirtualCamera instance.
     *
     * The camera is initially stopped.
     */
    constructor();

    /**
     * Indicates whether the virtual camera is currently running.
     */
    readonly isRunning: boolean;

    /** Start the virtual camera. */
    start(): void;

    /** Stops the virtual camera. */
    stop(): void;

    /**
     * Sends a new image frame to the virtual camera.
     * The image data must be linear RGBA with 8 bits per channel.
     *
     * @param {number} width - Image width in pixels.
     * @param {number} height - Image height in pixels.
     * @param {Uint8Array} image - Raw image data in linear RGBA format with 8 bits
     * per channel. The array length must equal `width * height * 4`.
     * @returns {boolean} `true` if the frame was accepted by the camera
     * (e.g., a process is actively consuming the frames). `false`
     * if the frame was not accepted (e.g., no application is currently
     * reading from the virtual camera).
     */
    send(width: number, height: number, image: Uint8Array): boolean;
}
