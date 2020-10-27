/**
 * TODO:
 *   - Derive sql type for Temperature and Humidity
 */
use sqlx::sqlite::SqlitePool;
use gpio_cdev::{Chip, LineRequestFlags, LineHandle};
use gpio_cdev::errors::Error as GpioError;
use std::thread::sleep;
use std::time::Duration;
use std::error;

mod domain;

#[derive(Debug, Copy, Clone)]
struct Temperature(f32);
#[derive(Debug, Copy, Clone)]
struct Humidity(f32);
#[derive(Debug, Copy, Clone)]
struct Voltage(f32);

const NAME: &str = "mushrust";
const DEVICE: &str = "/dev/gpiochip0";

const CHIP_SELECT_PIN: u32 = 23;
const CLOCK_PIN: u32 = 24;
const MOSI_PIN: u32 = 27;
const MISO_PIN: u32 = 4;

const HIGH: u8 = 1;
const LOW: u8 = 0;

const TEMPERATURE_CHANNEL: u8 = 0;
const HUMIDITY_CHANNEL: u8 = 1;

const SLEEP_TIME: Duration = Duration::from_secs(60);

struct SPI {
    chip_select: LineHandle,
    clock: LineHandle,
    mosi: LineHandle,
    miso: LineHandle,
}

type SPIResult<T> = Result<T, GpioError>;

impl SPI {
    fn new(device: &str, name: &str, chip_select_pin: u32, clock_pin: u32, mosi_pin: u32, miso_pin: u32) -> SPIResult<SPI> {
        let mut gpio = Chip::new(device)?;

        let chip_select = gpio
            .get_line(chip_select_pin)?
            .request(LineRequestFlags::OUTPUT, 0, name)?;
        let clock = gpio
            .get_line(clock_pin)?
            .request(LineRequestFlags::OUTPUT, 0, name)?;
        let mosi = gpio
            .get_line(mosi_pin)?
            .request(LineRequestFlags::OUTPUT, 0, name)?;
        let miso = gpio
            .get_line(miso_pin)?
            .request(LineRequestFlags::INPUT, 0, name)?;

        Ok(SPI { chip_select: chip_select, clock: clock, mosi: mosi, miso: miso })
    }

    fn toggle_select(&self) -> SPIResult<()> {
        self.chip_select.set_value(HIGH)?;
        self.chip_select.set_value(LOW)?;
        Ok(())
    }

    fn write(&self, bit: u8) -> SPIResult<()> {
        self.clock.set_value(LOW)?;
        self.mosi.set_value(bit)?;
        self.clock.set_value(HIGH)?;
        Ok(())
    }

    fn read(&self) -> SPIResult<u8> {
        self.clock.set_value(LOW)?;
        let value = self.miso.get_value()?;
        self.clock.set_value(HIGH)?;
        Ok(value)
    }
}

#[async_std::main]
async fn main() -> Result<(), Box<dyn error::Error>> {
    let pool = SqlitePool::connect("mushrooms.db").await?;
    let spi = SPI::new(DEVICE, NAME, CHIP_SELECT_PIN, CLOCK_PIN, MOSI_PIN, MISO_PIN)?;

    println!("Starting mushroom monitoring");

    loop {
        let temperature_input = adc_read(&spi, TEMPERATURE_CHANNEL)?;
        let temperature_voltage = adc_to_voltage(temperature_input);
        let temperature = voltage_to_temperature(temperature_voltage);

        let humidity_input = adc_read(&spi, HUMIDITY_CHANNEL)?;
        let humidity_voltage = adc_to_voltage(humidity_input);
        let humidity = voltage_to_humidity(humidity_voltage);

        println!("{:?} , {:?}", temperature, humidity);

        sqlx::query!("insert into measurements (at, temperature, humidity) values (datetime(\"now\"), ?, ?)", temperature.0, humidity.0).execute(&pool).await?;

        sleep(SLEEP_TIME);
    }

}

fn adc_read(
    spi: &SPI,
    channel: u8,
) -> Result<u16, GpioError> {
    spi.toggle_select()?;

    // Start bit(1)
    spi.write(1)?;
    // Mode selector (1 == Single channel)
    spi.write(1)?;
    // Channel selection
    spi.write(channel)?;
    // MSB mode(1 == MSB only)
    spi.write(1)?;

    // Ignore leading null bit
    spi.read()?;

    let mut input = 0;
    let mut idx = 9;

    while idx >= 0 {
        let current_bit = spi.read()? as u16;

        input = input | (current_bit << idx);

        idx = idx - 1;
    }

    Ok(input)
}

fn voltage_to_humidity(voltage: Voltage) -> Humidity {
    Humidity((voltage.0 - 0.86) / 0.03)
}

fn voltage_to_temperature(voltage: Voltage) -> Temperature {
    Temperature((voltage.0 - 0.5) * 100.0)
}

fn adc_to_voltage(raw: u16) -> Voltage {
    Voltage(raw as f32 * 0.0045898)
}
