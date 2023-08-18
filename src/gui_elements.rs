#[derive(Debug, PartialEq)]
pub enum Salesman {
    Jake,
    Danny
}
#[derive(Debug, PartialEq)]
pub enum Techs{
    Logan,
    Bread,
    Taco
}
#[derive(Debug, PartialEq)]

pub enum HardwareTest{
    RamPass,
    RamFail,
    RamNotTested,
    HddPass,
    HddFail,
    HddNotTested,
    SsdPass,
    SsdFail,
    SsdNotTested,
}


impl HardwareTest{
    pub fn as_str(&self) -> &'static str {
        match *self {
            HardwareTest::RamPass => "RAM Pass",
            HardwareTest::RamFail => "RAM Fail",
            HardwareTest::HddPass => "HDD Pass",
            HardwareTest::HddFail => "HDD Fail",
            HardwareTest::SsdPass => "SSD Pass",
            HardwareTest::SsdFail => "SSD Fail",
            HardwareTest::RamNotTested => "RAM not tested",
            HardwareTest::HddNotTested => "HDD not tested",
            HardwareTest::SsdNotTested => "SSD not tested",
        }
    }
}
