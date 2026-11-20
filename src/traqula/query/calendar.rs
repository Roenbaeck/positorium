//! Typed calendar expressions in WHERE. Computed intervals are query-local.
use super::*;
use crate::datatype::TimeInterval;

#[derive(Debug, Clone)]
enum DateExpr {
    Operand(Operand),
    Shift {
        date: Box<DateExpr>,
        days: Operand,
        subtract: bool,
    },
}

#[derive(Debug, Clone)]
pub(super) struct Test {
    pub(super) source: String,
    pub(super) variables: Vec<String>,
    operation: String,
    start: DateExpr,
    end: DateExpr,
    comparison: Option<(String, Operand)>,
}

fn comparison(message: impl Into<String>) -> DatabaseError {
    DatabaseError::Comparison(message.into())
}

impl DateExpr {
    fn parse(pair: Pair<'_, Rule>, execution: &ExecutionContext) -> Result<Self, DatabaseError> {
        let mut parts = pair.into_inner();
        let first = parts.next().unwrap();
        if first.as_rule() == Rule::day_shift {
            let date = Box::new(Self::parse(parts.next().unwrap(), execution)?);
            let days = parse_operand(
                parts.next().unwrap().into_inner().next().unwrap(),
                execution,
            )?;
            return Ok(Self::Shift {
                date,
                days,
                subtract: first.as_str().eq_ignore_ascii_case("subtract_days"),
            });
        }
        Ok(Self::Operand(parse_operand(first, execution)?))
    }

    fn validate(&self, domains: &HashMap<String, Domain>) -> Result<(), DatabaseError> {
        match self {
            Self::Operand(value) => validate(value, domains, true),
            Self::Shift { date, days, .. } => {
                date.validate(domains)?;
                validate(days, domains, false)
            }
        }
    }

    fn interval(&self, binding: &Binding, now: &Time) -> Result<TimeInterval, DatabaseError> {
        let value = match self {
            Self::Operand(value) => value,
            Self::Shift {
                date,
                days,
                subtract,
            } => {
                let n = integer(days, binding)?;
                let days = if *subtract {
                    n.checked_neg()
                        .ok_or_else(|| comparison("calendar-day offset overflow"))?
                } else {
                    n
                };
                return date
                    .interval(binding, now)?
                    .shift_days(days)
                    .map_err(comparison);
            }
        };
        let from_literal = |literal: &LiteralValue| {
            if literal.family() == LiteralFamily::Time
                && let Some(time) = parse_time_with_now(literal.token(), now)
            {
                return Ok(time.interval());
            }
            Err(comparison(format!(
                "expected a calendar date, found {}",
                literal.token()
            )))
        };
        match value {
            Operand::Time(time) => Ok(time.interval()),
            Operand::Literal(literal) => from_literal(literal),
            Operand::Variable(name) => match binding.get(name) {
                Some(BoundValue::Time(time)) => Ok(time.interval()),
                Some(BoundValue::Literal(literal)) => from_literal(literal),
                _ => Err(comparison(format!("?{name} must hold a date"))),
            },
            Operand::Identity(_) => Err(comparison("calendar operation requires a date")),
            Operand::Parameter(name) => {
                Err(comparison(format!("unbound calendar parameter {name}")))
            }
        }
    }
}

fn validate(
    operand: &Operand,
    domains: &HashMap<String, Domain>,
    date: bool,
) -> Result<(), DatabaseError> {
    let compatible = match operand {
        Operand::Variable(name) => {
            require_variable(domains, name)?;
            domains[name] == Domain::Literal || (date && domains[name] == Domain::Time)
        }
        Operand::Identity(_) => false,
        Operand::Parameter(_) => true,
        Operand::Time(_) => date,
        Operand::Literal(value) => {
            if date {
                value.family() == LiteralFamily::Time
            } else {
                value.family() == LiteralFamily::Integer
            }
        }
    };
    if compatible {
        Ok(())
    } else {
        Err(comparison(if date {
            "calendar operation requires a date"
        } else {
            "day count requires an integer"
        }))
    }
}

fn integer(operand: &Operand, binding: &Binding) -> Result<i64, DatabaseError> {
    let literal = match operand {
        Operand::Variable(name) => match binding.get(name) {
            Some(BoundValue::Literal(value)) => Some(value),
            _ => None,
        },
        Operand::Literal(value) => Some(value),
        _ => None,
    };
    literal
        .filter(|v| v.family() == LiteralFamily::Integer)
        .and_then(|v| v.token().parse().ok())
        .ok_or_else(|| comparison("day count requires a signed 64-bit integer"))
}

impl Test {
    pub(super) fn parse(
        pair: Pair<'_, Rule>,
        execution: &ExecutionContext,
    ) -> Result<Self, DatabaseError> {
        let source = pair.as_str().to_owned();
        let variables = pair
            .clone()
            .into_inner()
            .flatten()
            .filter(|p| p.as_rule() == Rule::query_variable)
            .map(|p| variable_name(&p))
            .collect();
        let mut parts = pair.into_inner();
        let operation = parts.next().unwrap().as_str().to_ascii_lowercase();
        let start = DateExpr::parse(parts.next().unwrap(), execution)?;
        let end = DateExpr::parse(parts.next().unwrap(), execution)?;
        let comparison = if let Some(operator) = parts.next() {
            if !matches!(operator.as_str(), "=" | "===" | "<" | "<=" | ">" | ">=") {
                return Err(comparison("unsupported elapsed-day comparison"));
            }
            Some((
                operator.as_str().to_owned(),
                parse_operand(
                    parts.next().unwrap().into_inner().next().unwrap(),
                    execution,
                )?,
            ))
        } else {
            None
        };
        Ok(Self {
            source,
            variables,
            operation,
            start,
            end,
            comparison,
        })
    }

    pub(super) fn validate(&self, domains: &HashMap<String, Domain>) -> Result<(), DatabaseError> {
        self.start.validate(domains)?;
        self.end.validate(domains)?;
        if let Some((_, count)) = &self.comparison {
            validate(count, domains, false)?;
        }
        Ok(())
    }

    pub(super) fn evaluate(&self, binding: &Binding, now: &Time) -> Result<bool, DatabaseError> {
        let start = self.start.interval(binding, now)?;
        let end = self.end.interval(binding, now)?;
        if let Some((op, count)) = &self.comparison {
            let (min, max) = start.elapsed_days(end).map_err(comparison)?;
            let days = if self.operation == "min_days_between" {
                min
            } else {
                max
            };
            let count = integer(count, binding)?;
            return Ok(match op.as_str() {
                "<" => days < count,
                "<=" => days <= count,
                ">" => days > count,
                ">=" => days >= count,
                _ => days == count,
            });
        }
        Ok(start.relation(&self.operation, end))
    }
}
