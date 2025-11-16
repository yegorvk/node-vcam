/**
 * Represents a virtual camera device.
 */
export declare class VirtualCamera {
    /**
     * Creates a new VirtualCamera instance.
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
     * @param {number} width - The width of the image in pixels.
     * @param {number} height - The height of the image in pixels.
     * @param {Uint8Array} image - The raw image data as a byte array,
     * formatted as linear RGBA with 8 bits per channel.
     * @returns {boolean} `true` if the frame was accepted by the camera
     * (e.g., a process is actively consuming the frames). Returns `false`
     * if the frame was not accepted (e.g., no application is currently
     * reading from the virtual camera).
     */
    send(width: number, height: number, image: Uint8Array): boolean;
}
