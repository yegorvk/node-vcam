function resolveModulePath(relativePath) {
    return new URL("./" + relativePath, import.meta.url);
}

const isDebug = Object.hasOwn(process.env, "NODE_VCAM_DEBUG");
const modulePath = isDebug ? "build/debug/index.js" : "build/release/index.js";

const module = await import(resolveModulePath(modulePath));
export const VirtualCamera = module.VirtualCamera;
