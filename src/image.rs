use snafu::Snafu;

#[derive(Debug, Copy, Clone)]
pub struct ImageSpec {
    pub extent: ImageExtent,
    pub format: PixelFormat,
}

impl ImageSpec {
    pub const fn new(extent: ImageExtent, format: PixelFormat) -> Self {
        Self { extent, format }
    }

    pub const fn size_in_bytes(&self) -> usize {
        self.format.size_in_bytes() * self.extent.area()
    }
}

// Ensures that `ImageSpec::size_in_bytes` never overflows.
const _: usize = ImageSpec::new(ImageExtent::MAX, PixelFormat::Rgba8Linear).size_in_bytes();

#[derive(Debug, Copy, Clone)]
pub enum PixelFormat {
    Rgba8Linear,
}

impl PixelFormat {
    const fn size_in_bytes(&self) -> usize {
        match self {
            PixelFormat::Rgba8Linear => 4 * size_of::<u8>(),
        }
    }
}

#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub struct ImageExtent {
    width: u32,
    height: u32,
}

impl ImageExtent {
    pub const MAX: Self = Self {
        width: 3840,
        height: 2160,
    };

    pub fn new(width: u32, height: u32) -> Result<Self, ImageExtentError> {
        if width == 0 {
            return Err(ImageExtentError::ZeroWidth);
        }

        if height == 0 {
            return Err(ImageExtentError::ZeroHeight);
        }

        if width > ImageExtent::MAX.width {
            return Err(ImageExtentError::WidthTooLarge { width });
        }

        if height > ImageExtent::MAX.height {
            return Err(ImageExtentError::HeightTooLarge { height });
        }

        Ok(Self { width, height })
    }

    pub const fn width(&self) -> u32 {
        self.width
    }

    pub const fn height(&self) -> u32 {
        self.height
    }

    const fn area(&self) -> usize {
        self.width as usize * self.height as usize
    }
}

// Ensures that `ImageExtent::area` never overflows.
const _: usize = ImageExtent::MAX.area();

#[derive(Debug, Snafu)]
#[snafu(module)]
pub enum ImageExtentError {
    #[snafu(display("image width must not be zero"))]
    ZeroWidth,

    #[snafu(display("image height must not be zero"))]
    ZeroHeight,

    #[snafu(display("image width (={width}) must not exceed {}", ImageExtent::MAX.width()))]
    WidthTooLarge { width: u32 },

    #[snafu(display("image height (={height}) must not exceed {}", ImageExtent::MAX.height()))]
    HeightTooLarge { height: u32 },
}

pub struct Image<'a> {
    spec: ImageSpec,
    data: &'a [u8],
}

impl<'a> Image<'a> {
    pub fn new(spec: ImageSpec, data: &'a [u8]) -> Result<Self, CreateImageError> {
        let image_size_in_bytes = spec.size_in_bytes();

        if image_size_in_bytes != data.len() {
            return Err(CreateImageError::DataSizeMismatch {
                expected: image_size_in_bytes,
                actual: data.len(),
            });
        }

        Ok(Self { spec, data })
    }

    pub fn spec(&'a self) -> &'a ImageSpec {
        &self.spec
    }

    pub fn data(&'a self) -> &'a [u8] {
        self.data
    }
}

#[derive(Debug, Snafu)]
#[snafu(module)]
pub enum CreateImageError {
    #[snafu(display("Image data size mismatch (expected {expected} bytes, found {actual})."))]
    DataSizeMismatch { expected: usize, actual: usize },
}
