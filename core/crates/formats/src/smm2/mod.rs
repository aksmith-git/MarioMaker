//! Super Mario Maker 2 course data and `.bcd` wrapper.

pub mod clear;
pub mod course;
pub mod crypto;
pub mod keyfind;

pub use course::{Area, AreaHeader, COURSE_SIZE, Course, CourseHeader, CourseObject};
pub use crypto::{BCD_SIZE, BcdFile, KeyTable};
