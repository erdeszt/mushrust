/**
 * TODO:
 *   - Split up this file
 *   - Restore fan cycles once circuit works
 *   - Derive sql type for Temperature and Humidity
 */
#[macro_use]
extern crate derive_more;

use sqlx::sqlite::SqlitePool;
use gpio_cdev::{Chip, LineRequestFlags, LineHandle};
use gpio_cdev::errors::Error as GpioError;
use std::thread::sleep;
use std::time::Duration;
use std::error;
use std::cmp::Ordering;
use warp::Filter;

mod domain;

use domain::Measurement;

#[derive(Debug, Copy, Clone, Add, Div, AddAssign)]
struct Temperature(f32);
#[derive(Debug, Copy, Clone, PartialEq, Add, Div, AddAssign)]
struct Humidity(f32);
#[derive(Debug, Copy, Clone)]
struct Voltage(f32);

impl Eq for Humidity { }

impl PartialOrd for Humidity {
    fn partial_cmp(&self, other: &Humidity) -> Option<Ordering> {
        self.0.partial_cmp(&other.0)
    }
}

enum FanState {
    STOPPED,
    ON(i32),
    COOLDOWN(i32),
}

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

#[derive(Debug)]
struct SqliteError(sqlx::Error);

impl warp::reject::Reject for SqliteError {}

const NAME: &str = "mushrust";
const DEVICE: &str = "/dev/gpiochip0";

const CHIP_SELECT_PIN: u32 = 23;
const CLOCK_PIN: u32 = 24;
const MOSI_PIN: u32 = 27;
const MISO_PIN: u32 = 4;

const WARNING_PIN: u32 = 22;
const WARNING_RESET_PIN: u32 = 17;

const FAN_PIN: u32 = 18;

const HIGH: u8 = 1;
const LOW: u8 = 0;

const TEMPERATURE_CHANNEL: u8 = 0;
const HUMIDITY_CHANNEL: u8 = 1;

const SLEEP_TIME: Duration = Duration::from_secs(60);

const SAMPLE_SIZE: usize = 3;

const HUMIDITY_VOLTAGE_OFFSET: f32 = 0.86;
const HUMIDITY_MAX: Humidity = Humidity(75f32);
const HUMIDITY_MIN: Humidity = Humidity(65f32);

const FAN_MAX_CYCLE: i32 = 1; // TODO: Should be 3
const FAN_COOLDOWN_CYCLE: i32 = 1; // TODO: Should be 3


async fn serve_measurements_last_two_hours(pool: sqlx::Pool<sqlx::Sqlite>) -> Result<impl warp::Reply, warp::Rejection> {
    let measurements = sqlx::query_as!(Measurement, "select * from measurements order by at desc limit 120")
        .fetch_all(&pool)
        .await
        .map_err(|error| warp::reject::custom(SqliteError(error)))?;

    Ok(warp::reply::json(&measurements))
}

async fn serve_measurements_last_two_days_hourly(pool: sqlx::Pool<sqlx::Sqlite>) -> Result<impl warp::Reply, warp::Rejection> {
    let measurements = sqlx::query_as!(
            Measurement,
            "select * from measurements
             where strftime('%M', at) == '00'
             order by at desc
             limit 48"
        )
        .fetch_all(&pool)
        .await
        .map_err(|error| warp::reject::custom(SqliteError(error)))?;

    Ok(warp::reply::json(&measurements))
}

async fn serve_measurements_all_time_daily(pool: sqlx::Pool<sqlx::Sqlite>) -> Result<impl warp::Reply, warp::Rejection> {
    let measurements = sqlx::query_as!(
            Measurement,
            "select * from measurements
             where strftime('%H%M', at) == '1200'
             order by at desc"
        )
        .fetch_all(&pool)
        .await
        .map_err(|error| warp::reject::custom(SqliteError(error)))?;

    Ok(warp::reply::json(&measurements))
}

fn with_pool(pool: sqlx::Pool<sqlx::Sqlite>) -> impl Filter<Extract = (sqlx::Pool<sqlx::Sqlite>,), Error = std::convert::Infallible> + Clone {
    warp::any().map(move || pool.clone())
}

async fn start_server(pool: sqlx::Pool<sqlx::Sqlite>) -> Result<(), sqlx::Error> {
    let index_route = warp::get()
        .and(warp::path::end())
        .and(warp::fs::file("./ui/index.html"));
    let measurements_route_last_two_hours = warp::path!("measurements" / "last_two_hours")
        .and(with_pool(pool.clone()))
        .and_then(serve_measurements_last_two_hours);
    let measurements_route_last_two_days_hourly = warp::path!("measurements" / "last_two_days_hourly")
        .and(with_pool(pool.clone()))
        .and_then(serve_measurements_last_two_days_hourly);
    let measurements_route_all_time_daily = warp::path!("measurements" / "all_time_daily")
        .and(with_pool(pool.clone()))
        .and_then(serve_measurements_all_time_daily);
    let routes = index_route
        .or(measurements_route_last_two_hours)
        .or(measurements_route_last_two_days_hourly)
        .or(measurements_route_all_time_daily);

    warp::serve(routes)
        .run(([0, 0, 0, 0], 3030))
        .await;

    Ok(())
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn error::Error>> {
    let mut fan_state = FanState::STOPPED;
    let pool = SqlitePool::connect("mushrooms.db").await?;

    tokio::task::spawn(start_server(pool.clone()));

    let mut chip = Chip::new(DEVICE)?;
    let spi = SPI::new(&mut chip, NAME, CHIP_SELECT_PIN, CLOCK_PIN, MOSI_PIN, MISO_PIN)?;
    let warning_pin = chip
        .get_line(WARNING_PIN)?
        .request(LineRequestFlags::OUTPUT, 0, NAME)?;
    let warning_reset_pin = chip
        .get_line(WARNING_RESET_PIN)?
        .request(LineRequestFlags::INPUT, 0, NAME)?;
    let fan_enable = chip
        .get_line(FAN_PIN)?
        .request(LineRequestFlags::OUTPUT, 0, NAME)?;

    println!("Starting mushroom monitoring");

    loop {
        let temperature = read_temperature(&spi, &warning_pin)?;
        let humidity = read_humidity(&spi, &warning_pin)?;
        let warning_reset = warning_reset_pin.get_value()?;

        if warning_reset == HIGH {
            warning_pin.set_value(LOW)?;
        }

        println!("Measurement: {:?} , {:?}", temperature, humidity);

        sqlx::query!("insert into measurements (at, temperature, humidity) values (datetime(\"now\"), ?, ?)", temperature.0, humidity.0).execute(&pool).await?;

        match fan_state {
            FanState::STOPPED => {
                if humidity > HUMIDITY_MAX {
                    println!("Humidity is too high, turning on fan");
                    fan_enable.set_value(HIGH)?;
                    fan_state = FanState::ON(0);
                }
            }
            FanState::ON(cycle) => {
                if cycle >=  FAN_MAX_CYCLE || humidity < HUMIDITY_MIN {
                    println!("Turning fan off");
                    fan_enable.set_value(LOW)?;
                    fan_state = FanState::COOLDOWN(0);
                }
                else {
                    fan_state = FanState::ON(cycle + 1);
                }
            }
            FanState::COOLDOWN(cycle) => {
                if cycle >= FAN_COOLDOWN_CYCLE {
                    println!("Fan cooled down, ready for new run");
                    fan_state = FanState::STOPPED;
                }
                else {
                    fan_state = FanState::COOLDOWN(cycle + 1);
                }
            }
        }

        sleep(SLEEP_TIME);
    }

}

fn read_temperature(spi: &SPI, warning_pin: &LineHandle) -> Result<Temperature, GpioError> {
    let mut sample_temperature_sum = Temperature(0f32);
    let mut valid_sample_count = 0;

    for _ in 0..SAMPLE_SIZE {
        let sample_temperature = voltage_to_temperature(
            adc_to_voltage(adc_read(spi, TEMPERATURE_CHANNEL)?)
        );

        if sample_temperature.0 < 0f32  || sample_temperature.0 > 40f32 {
            println!("WARNING: Invalid temperature: {:?}", sample_temperature);
            warning_pin.set_value(HIGH)?;
        }
        else {
            valid_sample_count += 1;
            sample_temperature_sum += sample_temperature;
        }
    }

    if valid_sample_count == 0 {
        Ok(Temperature(0f32))
    } else {
        let average_temperature = Temperature(sample_temperature_sum.0 / valid_sample_count as f32);

        Ok(average_temperature)
    }
}


fn read_humidity(spi: &SPI, warning_pin: &LineHandle) -> Result<Humidity, GpioError> {
    let mut sample_humidity_sum = Humidity(0f32);
    let mut valid_sample_count = 0;

    for _ in 0..SAMPLE_SIZE {
        let sample_humidity = voltage_to_humidity(
            adc_to_voltage(adc_read(spi, HUMIDITY_CHANNEL)?)
        );

        if sample_humidity.0 < 0f32 || sample_humidity.0 > 100f32 {
            println!("WARNING: invalid humidity level: {:?}", sample_humidity);
            warning_pin.set_value(HIGH)?;
        }
        else {
            valid_sample_count += 1;
            sample_humidity_sum += sample_humidity;
        }
    }

    if valid_sample_count == 0 {
        Ok(Humidity(0f32))
    } else {
        let average_humidity = Humidity(sample_humidity_sum.0 / SAMPLE_SIZE as f32);

        Ok(average_humidity)
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
    Humidity((voltage.0 - HUMIDITY_VOLTAGE_OFFSET) / 0.03)
}

fn voltage_to_temperature(voltage: Voltage) -> Temperature {
    Temperature((voltage.0 - 0.5) * 100.0)
}

fn adc_to_voltage(raw: u16) -> Voltage {
    Voltage(raw as f32 * 0.0045898)
}
