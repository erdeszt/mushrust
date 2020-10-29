/**
 * TODO:
 *   - Warning for invalid temperatures
 *   - When humidity > 70 turn on fan
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

const WARNING_PIN: u32 = 22;
const WARNING_RESET_PIN: u32 = 17;

const HIGH: u8 = 1;
const LOW: u8 = 0;

const TEMPERATURE_CHANNEL: u8 = 0;
const HUMIDITY_CHANNEL: u8 = 1;

const SLEEP_TIME: Duration = Duration::from_secs(60);

const SAMPLE_SIZE: usize = 3;

const HUMIDITY_VOLTAGE_OFFSET: f32 = 0.86;

struct SPI {
    chip_select: LineHandle,
    clock: LineHandle,
    mosi: LineHandle,
    miso: LineHandle,
}

type SPIResult<T> = Result<T, GpioError>;

impl SPI {
    fn new(chip: &mut Chip, name: &str, chip_select_pin: u32, clock_pin: u32, mosi_pin: u32, miso_pin: u32) -> SPIResult<SPI> {

        let chip_select = chip
            .get_line(chip_select_pin)?
            .request(LineRequestFlags::OUTPUT, 0, name)?;
        let clock = chip
            .get_line(clock_pin)?
            .request(LineRequestFlags::OUTPUT, 0, name)?;
        let mosi = chip
            .get_line(mosi_pin)?
            .request(LineRequestFlags::OUTPUT, 0, name)?;
        let miso = chip
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
    let mut chip = Chip::new(DEVICE)?;
    let pool = SqlitePool::connect("mushrooms.db").await?;
    let spi = SPI::new(&mut chip, NAME, CHIP_SELECT_PIN, CLOCK_PIN, MOSI_PIN, MISO_PIN)?;
    let warning_pin = chip
        .get_line(WARNING_PIN)?
        .request(LineRequestFlags::OUTPUT, 0, NAME)?;
    let warning_reset_pin = chip
        .get_line(WARNING_RESET_PIN)?
        .request(LineRequestFlags::INPUT, 0, NAME)?;

    println!("Starting mushroom monitoring");

    loop {
        let temperature = read_temperature(&spi)?;
        let humidity = read_humidity(&spi, &warning_pin)?;
        let warning_reset = warning_reset_pin.get_value()?;

        if warning_reset == HIGH {
            warning_pin.set_value(LOW)?;
        }

        println!("Measurement: {:?} , {:?}", temperature, humidity);

        sqlx::query!("insert into measurements (at, temperature, humidity) values (datetime(\"now\"), ?, ?)", temperature.0, humidity.0).execute(&pool).await?;

        sleep(SLEEP_TIME);
    }

}

fn read_temperature(spi: &SPI) -> Result<Temperature, GpioError> {
    let mut sample_sum = 0;

    for _ in 0..SAMPLE_SIZE {
        sample_sum += adc_read(spi, TEMPERATURE_CHANNEL)?;
    }

    let sample_average = sample_sum / SAMPLE_SIZE as u16;
    let average_voltage = adc_to_voltage(sample_average);
    let average_temperature = voltage_to_temperature(average_voltage);

    Ok(average_temperature)
}

fn read_humidity(spi: &SPI, warning_pin: &LineHandle) -> Result<Humidity, GpioError> {
    let mut sample_voltage_sum = 0f32;

    for _ in 0..SAMPLE_SIZE {
        let sample = adc_read(spi, HUMIDITY_CHANNEL)?;
        let sample_voltage = adc_to_voltage(sample);

        if sample_voltage.0 < HUMIDITY_VOLTAGE_OFFSET {
            println!("WARNING: Negative humidity value for voltage level: {:?}", sample_voltage);
            warning_pin.set_value(HIGH)?;
        }
        else {
            sample_voltage_sum += sample_voltage.0;
        }
    }

    let average_voltage = sample_voltage_sum / SAMPLE_SIZE as f32;
    let humidity = voltage_to_humidity(Voltage(average_voltage));

    Ok(humidity)
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
    Humidity((voltage.0 - HUMIDITY_VOLTAGE_OFFSET) / 0.03)
}

fn voltage_to_temperature(voltage: Voltage) -> Temperature {
    Temperature((voltage.0 - 0.5) * 100.0)
}

fn adc_to_voltage(raw: u16) -> Voltage {
    Voltage(raw as f32 * 0.0045898)
}
