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
    pub w: u32,
    pub h: u32,
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
            w: (self.rb.x.max(self.lt.x) - self.lt.x) as u32,
            h: (self.rb.y.max(self.lt.y) - self.lt.y) as u32,
        }
    }

    pub fn contains(&self, pos: Pos) -> bool {
        pos.x >= self.lt.x && pos.x < self.rb.x && pos.y >= self.lt.y && pos.y < self.rb.y
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

#[cold]
#[track_caller]
fn divide_by_zero() -> ! {
    panic!("attempt to divide by zero");
}
