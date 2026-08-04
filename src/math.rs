use std::{
    num::NonZero,
    ops::{Add, AddAssign, Div, Mul, Neg, Sub, SubAssign},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Vec {
    pub x: i32,
    pub y: i32,
}

impl Vec {
    pub const ZERO: Vec = Vec { x: 0, y: 0 };
}

impl From<Pos> for Vec {
    #[inline]
    fn from(value: Pos) -> Self {
        Vec {
            x: value.x,
            y: value.y,
        }
    }
}

impl Add for Vec {
    type Output = Self;

    #[inline]
    fn add(self, rhs: Self) -> Self {
        Vec {
            x: self.x + rhs.x,
            y: self.y + rhs.y,
        }
    }
}

impl AddAssign for Vec {
    #[inline]
    fn add_assign(&mut self, rhs: Self) {
        self.x += rhs.x;
        self.y += rhs.y;
    }
}

impl Sub for Vec {
    type Output = Self;

    #[inline]
    fn sub(self, rhs: Self) -> Self {
        Vec {
            x: self.x - rhs.x,
            y: self.y - rhs.y,
        }
    }
}

impl SubAssign for Vec {
    #[inline]
    fn sub_assign(&mut self, rhs: Self) {
        self.x -= rhs.x;
        self.y -= rhs.y;
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Pos {
    pub x: i32,
    pub y: i32,
}

impl Pos {
    pub const ZERO: Pos = Pos { x: 0, y: 0 };
}

impl From<Vec> for Pos {
    #[inline]
    fn from(value: Vec) -> Self {
        Pos {
            x: value.x,
            y: value.y,
        }
    }
}

impl Add<Vec> for Pos {
    type Output = Self;

    #[inline]
    fn add(self, rhs: Vec) -> Self {
        Pos {
            x: self.x + rhs.x,
            y: self.y + rhs.y,
        }
    }
}

impl AddAssign<Vec> for Pos {
    #[inline]
    fn add_assign(&mut self, rhs: Vec) {
        self.x += rhs.x;
        self.y += rhs.y;
    }
}

impl Sub<Pos> for Pos {
    type Output = Vec;

    #[inline]
    fn sub(self, rhs: Pos) -> Vec {
        Vec {
            x: self.x - rhs.x,
            y: self.y - rhs.y,
        }
    }
}

impl SubAssign<Vec> for Pos {
    #[inline]
    fn sub_assign(&mut self, rhs: Vec) {
        self.x -= rhs.x;
        self.y -= rhs.y;
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Size {
    pub w: i32,
    pub h: i32,
}

impl Size {
    pub const ZERO: Size = Size { w: 0, h: 0 };
}

impl Add for Size {
    type Output = Self;

    #[inline]
    fn add(self, rhs: Self) -> Self {
        Size {
            w: self.w + rhs.w,
            h: self.h + rhs.h,
        }
    }
}

impl AddAssign for Size {
    #[inline]
    fn add_assign(&mut self, rhs: Self) {
        self.w += rhs.w;
        self.h += rhs.h;
    }
}

impl Size {
    pub fn fits(self, other: Size) -> bool {
        self.w >= other.w && self.h >= other.h
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Rect {
    pub lt: Pos,
    pub rb: Pos,
}

impl Rect {
    pub const ZERO: Rect = Rect {
        lt: Pos { x: 0, y: 0 },
        rb: Pos { x: 0, y: 0 },
    };

    pub fn intersects(&self, other: &Rect) -> bool {
        self.lt.x < other.rb.x
            && self.rb.x > other.lt.x
            && self.lt.y < other.rb.y
            && self.rb.y > other.lt.y
    }

    pub fn union(&self, other: &Rect) -> Rect {
        let lt = Pos {
            x: self.lt.x.min(other.lt.x),
            y: self.lt.y.min(other.lt.y),
        };
        let rb = Pos {
            x: self.rb.x.max(other.rb.x),
            y: self.rb.y.max(other.rb.y),
        };
        Rect { lt, rb }
    }

    pub fn size(&self) -> Size {
        Size {
            w: (self.rb.x.max(self.lt.x) - self.lt.x),
            h: (self.rb.y.max(self.lt.y) - self.lt.y),
        }
    }

    pub fn contains(&self, pos: Pos) -> bool {
        pos.x >= self.lt.x && pos.x < self.rb.x && pos.y >= self.lt.y && pos.y < self.rb.y
    }

    pub fn from_pos_size(pos: Pos, size: Size) -> Rect {
        Rect {
            lt: pos,
            rb: Pos {
                x: pos.x + size.w,
                y: pos.y + size.h,
            },
        }
    }
}

impl Add<Vec> for Rect {
    type Output = Self;

    #[inline]
    fn add(self, rhs: Vec) -> Self {
        Rect {
            lt: self.lt + rhs,
            rb: self.rb + rhs,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Ratio {
    /// Numerator of the rational number.
    /// Carries the sign.
    pub num: i32,

    /// Denominator of the rational number.
    /// Always positive.
    pub den: NonZero<i32>,
}

impl Ratio {
    pub const ZERO: Ratio = Ratio {
        num: 0,
        den: const { NonZero::new(1).unwrap() },
    };

    pub fn new(num: i32, den: NonZero<i32>) -> Self {
        let mut num = num;
        let mut den = den;

        if den.get() < 0 {
            num = -num;
            den = den.abs();
        }

        let (num, den) = reduce(num, den);

        Ratio { num, den }
    }

    pub fn inv(&self) -> Option<Self> {
        if self.num == 0 {
            return None;
        }

        Some(Ratio {
            num: self.den.get() as i32,
            den: NonZero::new(self.num.abs()).unwrap(),
        })
    }

    pub const fn int(value: i32) -> Self {
        Ratio {
            num: value,
            den: const { NonZero::new(1).unwrap() },
        }
    }

    pub const fn floor(&self) -> i32 {
        self.num / self.den.get()
    }
}

impl Neg for Ratio {
    type Output = Self;

    #[inline]
    fn neg(self) -> Self {
        Ratio {
            num: -self.num,
            den: self.den,
        }
    }
}

impl Add for Ratio {
    type Output = Self;

    #[inline]
    fn add(self, rhs: Self) -> Self {
        if self.num == 0 {
            return rhs;
        }
        if rhs.num == 0 {
            return self;
        }

        let dgcd = gcd(self.den.get(), rhs.den.get());

        let num = self.num * (rhs.den.get() / dgcd) + rhs.num * (self.den.get() / dgcd);
        let den = self.den.get() * (rhs.den.get() / dgcd);

        let (num, den) = reduce(num, NonZero::new(den).unwrap());
        Ratio { num, den }
    }
}

impl Add<i32> for Ratio {
    type Output = Self;

    #[inline]
    fn add(self, rhs: i32) -> Self {
        if self.num == 0 {
            return Ratio::int(rhs);
        }

        let num = self.num + rhs * self.den.get();
        let den = self.den.get();

        let (num, den) = reduce(num, NonZero::new(den).unwrap());
        Ratio { num, den }
    }
}

impl Add<Ratio> for i32 {
    type Output = Ratio;

    #[inline]
    fn add(self, rhs: Ratio) -> Ratio {
        if rhs.num == 0 {
            return Ratio::int(self);
        }

        let num = self * rhs.den.get() + rhs.num;
        let den = rhs.den.get();

        let (num, den) = reduce(num, NonZero::new(den).unwrap());
        Ratio { num, den }
    }
}

impl Sub for Ratio {
    type Output = Self;

    #[inline]
    fn sub(self, rhs: Self) -> Self {
        if self.num == 0 {
            return -rhs;
        }
        if rhs.num == 0 {
            return self;
        }

        let dgcd = gcd(self.den.get(), rhs.den.get());

        let num = self.num * (rhs.den.get() / dgcd) - rhs.num * (self.den.get() / dgcd);
        let den = self.den.get() * (rhs.den.get() / dgcd);

        let (num, den) = reduce(num, NonZero::new(den).unwrap());
        Ratio { num, den }
    }
}

impl Sub<i32> for Ratio {
    type Output = Self;

    #[inline]
    fn sub(self, rhs: i32) -> Self {
        if self.num == 0 {
            return Ratio::int(-rhs);
        }

        let num = self.num - rhs * self.den.get();
        let den = self.den.get();

        let (num, den) = reduce(num, NonZero::new(den).unwrap());
        Ratio { num, den }
    }
}

impl Sub<Ratio> for i32 {
    type Output = Ratio;

    #[inline]
    fn sub(self, rhs: Ratio) -> Ratio {
        if rhs.num == 0 {
            return Ratio::int(self);
        }

        let num = self * rhs.den.get() - rhs.num;
        let den = rhs.den.get();

        let (num, den) = reduce(num, NonZero::new(den).unwrap());
        Ratio { num, den }
    }
}

impl Mul for Ratio {
    type Output = Self;

    #[inline]
    fn mul(self, rhs: Self) -> Self {
        if self.num == 0 || rhs.num == 0 {
            return Ratio::int(0);
        }

        let gcd1 = gcd(self.num.abs(), rhs.den.get());
        let gcd2 = gcd(rhs.num.abs(), self.den.get());

        let num = (self.num / gcd1) * (rhs.num / gcd2);
        let den = (self.den.get() / gcd2) * (rhs.den.get() / gcd1);

        let den = NonZero::new(den).unwrap();
        Ratio { num, den }
    }
}

impl Mul<i32> for Ratio {
    type Output = Self;

    #[inline]
    fn mul(self, rhs: i32) -> Self {
        if self.num == 0 || rhs == 0 {
            return Ratio::int(0);
        }

        let gcd = gcd(self.den.get(), rhs.abs());

        let num = self.num * (rhs / gcd);
        let den = self.den.get() / gcd;

        let den = NonZero::new(den).unwrap();
        Ratio { num, den }
    }
}

impl Mul<Ratio> for i32 {
    type Output = Ratio;

    #[inline]
    fn mul(self, rhs: Ratio) -> Ratio {
        if self == 0 || rhs.num == 0 {
            return Ratio::int(0);
        }

        let gcd = gcd(self.abs(), rhs.den.get());

        let num = (self / gcd) * rhs.num;
        let den = rhs.den.get() / gcd;

        let den = NonZero::new(den).unwrap();
        Ratio { num, den }
    }
}

impl Div for Ratio {
    type Output = Self;

    #[inline]
    fn div(self, rhs: Self) -> Self {
        if rhs.num == 0 {
            divide_by_zero();
        }
        if self.num == 0 {
            return Ratio::int(0);
        }

        let gcd1 = gcd(self.num.abs(), rhs.num.abs());
        let gcd2 = gcd(rhs.den.get(), self.den.get());

        let num = (self.num / gcd1) * (rhs.den.get() / gcd2);
        let den = (self.den.get() / gcd2) * (rhs.num.abs() / gcd1);

        let den = NonZero::new(den).unwrap();
        Ratio { num, den }
    }
}

impl Div<i32> for Ratio {
    type Output = Self;

    #[inline]
    fn div(self, rhs: i32) -> Self {
        if rhs == 0 {
            divide_by_zero();
        }
        if self.num == 0 {
            return Ratio::int(0);
        }

        let gcd = gcd(self.num.abs(), rhs.abs());

        let num = self.num / gcd;
        let den = self.den.get() * (rhs.abs() / gcd);

        let den = NonZero::new(den).unwrap();
        Ratio { num, den }
    }
}

impl Div<Ratio> for i32 {
    type Output = Ratio;

    #[inline]
    fn div(self, rhs: Ratio) -> Ratio {
        if rhs.num == 0 {
            divide_by_zero();
        }
        if self == 0 {
            return Ratio::int(0);
        }

        let gcd = gcd(self.abs(), rhs.num.abs());

        let num = self / gcd;
        let den = rhs.den.get() * (rhs.num.abs() / gcd);

        let den = NonZero::new(den).unwrap();
        Ratio { num, den }
    }
}

const fn reduce(num: i32, den: NonZero<i32>) -> (i32, NonZero<i32>) {
    if num == 0 {
        return (0, const { NonZero::new(1).unwrap() });
    }
    let gcd = gcd(num.abs(), den.get());
    (num / gcd, NonZero::new(den.get() / gcd).unwrap())
}

const fn gcd(a: i32, b: i32) -> i32 {
    if a <= 0 {
        panic!("gcd: a must be positive");
    }
    if b <= 0 {
        panic!("gcd: b must be positive");
    }

    let mut x = a;
    let mut y = b;
    while y != 0 {
        let t = y;
        y = x % y;
        x = t;
    }

    debug_assert!(a % x == 0);
    debug_assert!(b % x == 0);

    x
}

/// Relative position in fraction of a size.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct RelPos {
    pub x: Ratio,
    pub y: Ratio,
}

impl RelPos {
    pub const ZERO: RelPos = RelPos {
        x: Ratio::ZERO,
        y: Ratio::ZERO,
    };

    /// Create a relative position from an absolute position and size.
    pub fn from_absolute(pos: Pos, size: Size) -> Self {
        assert_ne!(size.w, 0, "size.w must not be zero");
        assert_ne!(size.h, 0, "size.h must not be zero");

        let x = Ratio::new(pos.x, NonZero::new(size.w).unwrap());
        let y = Ratio::new(pos.y, NonZero::new(size.h).unwrap());

        RelPos { x, y }
    }

    /// Convert a relative position to an absolute position given a size.
    pub fn into_absolute(self, size: Size) -> Pos {
        let x = (self.x.num * size.w) / self.x.den.get();
        let y = (self.y.num * size.h) / self.y.den.get();
        Pos { x, y }
    }
}

impl Add for RelPos {
    type Output = Self;

    #[inline]
    fn add(self, rhs: Self) -> Self {
        RelPos {
            x: self.x + rhs.x,
            y: self.y + rhs.y,
        }
    }
}

impl Sub for RelPos {
    type Output = Self;

    #[inline]
    fn sub(self, rhs: Self) -> Self {
        RelPos {
            x: self.x - rhs.x,
            y: self.y - rhs.y,
        }
    }
}

#[cold]
#[track_caller]
fn divide_by_zero() -> ! {
    panic!("attempt to divide by zero");
}
