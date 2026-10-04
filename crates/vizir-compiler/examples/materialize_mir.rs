//! Execute a derived numeric MIR binding while retaining explicit scale domains.
//! Run with an existing .viz.yaml chart document; emits the refreshed Scene2D JSON.
use vizir_compiler::{build_scene, lower_to_mir, rematerialize_mir};
use vizir_core::{ChartMark, Expression, LiteralValue, MirView, ValueType, parse_document};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = std::env::args()
        .nth(1)
        .ok_or("provide an input .viz.yaml chart document")?;
    let mut mir = lower_to_mir(&parse_document(path)?)?;
    let chart = mir
        .views
        .iter()
        .find_map(|view| match view {
            MirView::Chart(chart) => Some(chart),
            _ => None,
        })
        .ok_or("the input needs at least one chart")?;
    let expression_id = match &chart.mark {
        ChartMark::Symbol { y, .. } | ChartMark::Line { y, .. } => &y.expression,
        ChartMark::Bar { value, .. } => &value.expression,
    }
    .clone();
    let expression = mir
        .expressions
        .get_mut(&expression_id)
        .expect("normalized binding");
    expression.expression = Expression::Multiply {
        left: Box::new(expression.expression.clone()),
        right: Box::new(Expression::Literal {
            value: LiteralValue::Float64(0.5),
        }),
    };
    expression.result_type = ValueType::Float64;
    // Stored instances may now be stale. Refresh explicitly; the scale domains
    // remain those of the original normalization, so the geometry visibly moves.
    let refreshed = rematerialize_mir(&mir)?;
    let scene = build_scene(&refreshed)?;
    println!("{}", serde_json::to_string_pretty(&scene)?);
    Ok(())
}
