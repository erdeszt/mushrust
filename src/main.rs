/**
 * TODO: 
 *   - SPI struct
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

#[async_std::main]
async fn main() -> Result<(), Box<dyn error::Error>> {
    let pool = SqlitePool::connect("mushrooms.db").await?;

    let mut gpio = Chip::new(DEVICE)?;

    let chip_select = gpio
        .get_line(CHIP_SELECT_PIN)?
        .request(LineRequestFlags::OUTPUT, 0, NAME)?;
    let clock = gpio
        .get_line(CLOCK_PIN)?
        .request(LineRequestFlags::OUTPUT, 0, NAME)?;
    let mosi = gpio
        .get_line(MOSI_PIN)?
        .request(LineRequestFlags::OUTPUT, 0, NAME)?;
    let miso = gpio
        .get_line(MISO_PIN)?
        .request(LineRequestFlags::INPUT, 0, NAME)?;

    println!("Starting mushroom monitoring");

    loop {
        let temperature_input = adc_read(
            &chip_select, 
            &clock,
            &mosi,
            &miso,
            0,
        )?;
        let temperature_voltage = adc_to_voltage(temperature_input);
        let temperature = voltage_to_temperature(temperature_voltage);

        let humidity_input = adc_read(
            &chip_select, 
            &clock,
            &mosi,
            &miso,
            1,
        )?;
        let humidity_voltage = adc_to_voltage(humidity_input);
        let humidity = voltage_to_humidity(humidity_voltage);

        println!("{:?} , {:?}", temperature, humidity);

        sqlx::query!("insert into measurements (at, temperature, humidity) values (datetime(\"now\"), ?, ?)", temperature.0, humidity.0).execute(&pool).await?;

        sleep(Duration::from_secs(60));
    }

}

fn adc_read(
    chip_select: &LineHandle, 
    clock: &LineHandle, 
    mosi: &LineHandle,
    miso: &LineHandle,
    channel: u8,
) -> Result<u16, GpioError> {
    spi_start(&chip_select)?;

    // Start bit(1)
    spi_out(&clock, &mosi, 1)?;
    // Mode selector (1 == Single channel)
    spi_out(&clock, &mosi, 1)?;
    // Channel selection
    spi_out(&clock, &mosi, channel)?;
    // MSB mode(1 == MSB only)
    spi_out(&clock, &mosi, 1)?;

    // Ignore leading null bit
    spi_in(&clock, &miso)?;

    let mut input = 0;
    let mut idx = 9;

    while idx >= 0 {
        let current_bit = spi_in(&clock, &miso)? as u16;

        input = input | (current_bit << idx);

        idx = idx - 1;
    }

    Ok(input)
}

fn spi_start(chip_select: &LineHandle) -> Result<(), GpioError> {
    chip_select.set_value(HIGH)?;
    chip_select.set_value(LOW)?;
    Ok(())
}

fn spi_out(clock: &LineHandle, mosi: &LineHandle, value: u8) -> Result<(), GpioError> {
    clock.set_value(LOW)?; 
    mosi.set_value(value)?;
    clock.set_value(HIGH)?;
    Ok(())
}

fn spi_in(clock: &LineHandle, miso: &LineHandle) -> Result<u8, GpioError> {
    clock.set_value(LOW)?;
    let value = miso.get_value()?;
    clock.set_value(HIGH)?;
    Ok(value)
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
