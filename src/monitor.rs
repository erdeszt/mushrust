use plotters::prelude::*;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let output = BitMapBackend::new("mushrooms.png", (640, 480)).into_drawing_area();
    
    output.fill(&WHITE);

    let mut chart = ChartBuilder::on(&output)
        .caption("Temperature and humidity", ("sans-serif", 50).into_font())
        .margin(5)
        .x_label_area_size(30)
        .y_label_area_size(30)
        .build_ranged(-1f32..1f32, -0.1f32..1f32)?;

    chart.configure_mesh().draw()?;

    chart
        .draw_series(LineSeries::new(
            (-50..50).map(|x| x as f32 / 50.0).map(|x| (x, x * x)),
            &RED,
        ))?
        .label("y = x ^ 2");

    chart
        .draw_series(LineSeries::new(
            (-50..50).map(|x| x as f32 / 50.0).map(|x| (x, 1f32 - x * x)),
            &BLUE,
        ))?
        .label("y = 1 - x ^ 2");

    chart
        .configure_series_labels()
        .background_style(&WHITE.mix(0.8))
        .border_style(&BLACK)
        .draw()?;

    println!("YO");

    Ok(())

}
