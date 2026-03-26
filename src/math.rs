/// All types here are repr(C) + Copy so they can be read directly from the
/// target process via Memory::read<T>.

#[repr(C)]
#[derive(Clone, Copy, Default, Debug, PartialEq)]
pub struct Vec3 {
    pub x: f32,
    pub y: f32,
    pub z: f32,
}

impl Vec3 {
    pub const ZERO: Self = Self { x: 0.0, y: 0.0, z: 0.0 };

    pub fn is_zero(&self) -> bool {
        self.x == 0.0 && self.y == 0.0 && self.z == 0.0
    }

    pub fn length(&self) -> f32 {
        (self.x * self.x + self.y * self.y + self.z * self.z).sqrt()
    }

    pub fn length_2d(&self) -> f32 {
        (self.x * self.x + self.y * self.y).sqrt()
    }

    pub fn dist_to(&self, other: &Vec3) -> f32 {
        (*self - *other).length()
    }

    pub fn dot(&self, other: &Vec3) -> f32 {
        self.x * other.x + self.y * other.y + self.z * other.z
    }

    pub fn normalized(&self) -> Self {
        let len = self.length() + f32::EPSILON;
        Self { x: self.x / len, y: self.y / len, z: self.z / len }
    }
}

impl std::ops::Add for Vec3 {
    type Output = Self;
    fn add(self, rhs: Self) -> Self {
        Self { x: self.x + rhs.x, y: self.y + rhs.y, z: self.z + rhs.z }
    }
}

impl std::ops::Sub for Vec3 {
    type Output = Self;
    fn sub(self, rhs: Self) -> Self {
        Self { x: self.x - rhs.x, y: self.y - rhs.y, z: self.z - rhs.z }
    }
}

impl std::ops::Mul<f32> for Vec3 {
    type Output = Self;
    fn mul(self, rhs: f32) -> Self {
        Self { x: self.x * rhs, y: self.y * rhs, z: self.z * rhs }
    }
}

// ─── Vec4 / VectorAligned ────────────────────────────────────────────────────

#[repr(C)]
#[derive(Clone, Copy, Default, Debug)]
pub struct Vec4 {
    pub x: f32,
    pub y: f32,
    pub z: f32,
    pub w: f32,
}

// ─── Matrix3x4 ───────────────────────────────────────────────────────────────
// C++ uses #pragma pack(push, 4) — all floats, so natural alignment == 4.

#[repr(C)]
#[derive(Clone, Copy, Default, Debug)]
pub struct Matrix3x4 {
    pub data: [[f32; 4]; 3],
}

impl Matrix3x4 {
    pub fn origin(&self) -> Vec3 {
        Vec3 {
            x: self.data[0][3],
            y: self.data[1][3],
            z: self.data[2][3],
        }
    }

    pub fn transform(&self, v: Vec3) -> Vec3 {
        Vec3 {
            x: self.data[0][0] * v.x + self.data[0][1] * v.y + self.data[0][2] * v.z + self.data[0][3],
            y: self.data[1][0] * v.x + self.data[1][1] * v.y + self.data[1][2] * v.z + self.data[1][3],
            z: self.data[2][0] * v.x + self.data[2][1] * v.y + self.data[2][2] * v.z + self.data[2][3],
        }
    }
}

// ─── ViewMatrix (4x4) ────────────────────────────────────────────────────────
// C++ uses #pragma pack(push, 4) — 64 bytes of f32.

#[repr(C)]
#[derive(Clone, Copy, Default, Debug)]
pub struct ViewMatrix {
    pub data: [[f32; 4]; 4],
}

impl ViewMatrix {
    /// Project a world-space position to screen-space (x, y) in [0..1].
    /// Returns None if behind the camera.
    pub fn world_to_screen(&self, world: Vec3, screen_w: f32, screen_h: f32) -> Option<(f32, f32)> {
        let m = &self.data;
        let w = m[3][0] * world.x + m[3][1] * world.y + m[3][2] * world.z + m[3][3];
        if w < 0.001 {
            return None;
        }
        let x = m[0][0] * world.x + m[0][1] * world.y + m[0][2] * world.z + m[0][3];
        let y = m[1][0] * world.x + m[1][1] * world.y + m[1][2] * world.z + m[1][3];
        let sx = screen_w / 2.0 + (x / w) * (screen_w / 2.0);
        let sy = screen_h / 2.0 - (y / w) * (screen_h / 2.0);
        Some((sx, sy))
    }
}

// ─── Bone data ───────────────────────────────────────────────────────────────

#[repr(C)]
#[derive(Clone, Copy, Default, Debug)]
pub struct Quaternion {
    pub x: f32,
    pub y: f32,
    pub z: f32,
    pub w: f32,
}

/// Matches C++ BoneData_t layout (Vec3 + f32 scale + Quaternion).
#[repr(C)]
#[derive(Clone, Copy, Default, Debug)]
pub struct BoneData {
    pub position: Vec3,
    pub scale: f32,
    pub rotation: Quaternion,
}

impl BoneData {
    /// Convert bone data to a 3x4 transform matrix (matches C++ TranslateToMatrix3x4).
    pub fn to_matrix(&self) -> Matrix3x4 {
        let q = &self.rotation;
        let p = &self.position;
        Matrix3x4 {
            data: [
                [
                    1.0 - 2.0 * q.y * q.y - 2.0 * q.z * q.z,
                    2.0 * q.x * q.y - 2.0 * q.w * q.z,
                    2.0 * q.x * q.z + 2.0 * q.w * q.y,
                    p.x,
                ],
                [
                    2.0 * q.x * q.y + 2.0 * q.w * q.z,
                    1.0 - 2.0 * q.x * q.x - 2.0 * q.z * q.z,
                    2.0 * q.y * q.z - 2.0 * q.w * q.x,
                    p.y,
                ],
                [
                    2.0 * q.x * q.z - 2.0 * q.w * q.y,
                    2.0 * q.y * q.z + 2.0 * q.w * q.x,
                    1.0 - 2.0 * q.x * q.x - 2.0 * q.y * q.y,
                    p.z,
                ],
            ],
        }
    }
}
