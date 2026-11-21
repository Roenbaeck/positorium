//! JSON Pointer expressions over the appearing value of a posit.
use super::*;
use crate::literal::JsonPointer;
use std::borrow::Cow;

#[derive(Debug, Clone)]
pub(super) struct Call {
    pub source: String,
    document: Operand,
    // A parameter is resolved at preparation. Only a saved definition being
    // checked for its schema can reach this stage without its pointer yet.
    pointer: Option<JsonPointer>,
    pub existence: bool,
}

fn validate_literal(
    operand: &Operand,
    domains: &HashMap<String, Domain>,
) -> Result<(), DatabaseError> {
    match operand {
        Operand::Variable(name) => {
            require_variable(domains, name)?;
            if domains[name] == Domain::Literal {
                return Ok(());
            }
        }
        Operand::Literal(_) | Operand::Parameter(_) => return Ok(()),
        _ => {}
    }
    Err(DatabaseError::Comparison(
        "JSON function requires a literal value".into(),
    ))
}

fn literal<'a>(
    operand: &'a Operand,
    binding: &'a Binding,
) -> Result<&'a LiteralValue, DatabaseError> {
    match operand {
        Operand::Literal(value) => Ok(value),
        Operand::Variable(name) => match binding.get(name) {
            Some(BoundValue::Literal(value)) => Ok(value),
            _ => Err(DatabaseError::UnknownVariable(name.clone())),
        },
        Operand::Parameter(name) => Err(DatabaseError::Parameter(format!(
            "unbound parameter {name}"
        ))),
        _ => Err(DatabaseError::Comparison(
            "JSON function requires a literal value".into(),
        )),
    }
}

impl Call {
    pub(super) fn parse(
        pair: Pair<'_, Rule>,
        execution: &ExecutionContext,
    ) -> Result<Self, DatabaseError> {
        let source = pair.as_str().to_owned();
        let existence = pair.as_rule() == Rule::json_exists;
        let mut args = pair.into_inner();
        let document_arg = args.next().unwrap().into_inner().next().unwrap();
        let parameter_document = document_arg.as_rule() == Rule::parameter;
        let document = parse_operand(document_arg, execution)?;
        if parameter_document
            && !execution.validating_definition
            && !matches!(&document, Operand::Literal(value) if value.family() == LiteralFamily::Json)
        {
            return Err(DatabaseError::Parameter(
                "JSON document parameter requires a JSON literal".into(),
            ));
        }
        let pointer = args.next().unwrap().into_inner().next().unwrap();
        let pointer = match parse_operand(pointer, execution)? {
            Operand::Parameter(_) => None,
            Operand::Literal(value) if value.family() == LiteralFamily::String => {
                let path: String = serde_json::from_str(value.token())
                    .map_err(|e| DatabaseError::Comparison(e.to_string()))?;
                Some(JsonPointer::parse(&path).map_err(DatabaseError::Comparison)?)
            }
            _ => {
                return Err(DatabaseError::Parameter(
                    "JSON Pointer requires a string literal".into(),
                ));
            }
        };
        Ok(Self {
            source,
            document,
            pointer,
            existence,
        })
    }

    pub(super) fn validate(&self, domains: &HashMap<String, Domain>) -> Result<(), DatabaseError> {
        validate_literal(&self.document, domains)
    }

    fn exists(&self, binding: &Binding) -> Result<bool, DatabaseError> {
        self.pointer()?
            .exists(literal(&self.document, binding)?)
            .map_err(DatabaseError::Comparison)
    }

    fn pointer(&self) -> Result<&JsonPointer, DatabaseError> {
        self.pointer.as_ref().ok_or_else(|| {
            DatabaseError::Invariant("JSON Pointer is unbound during evaluation".into())
        })
    }

    fn value(
        &self,
        binding: &Binding,
        memory: &mut Reservation,
    ) -> Result<Option<LiteralValue>, DatabaseError> {
        let token = if self.existence {
            Some(if self.exists(binding)? {
                "true"
            } else {
                "false"
            })
        } else {
            self.pointer()?
                .at(literal(&self.document, binding)?)
                .map_err(DatabaseError::Comparison)?
        };
        let Some(token) = token else { return Ok(None) };
        memory.grow(token.len() as u64)?;
        Ok(Some(LiteralValue::from_json_fragment(token)))
    }
}

#[derive(Debug, Clone)]
enum Expr {
    Lookup(Call),
    Value(Operand),
}

impl Expr {
    fn parse(pair: Pair<'_, Rule>, execution: &ExecutionContext) -> Result<Self, DatabaseError> {
        match pair.as_rule() {
            Rule::json_at | Rule::json_exists => Call::parse(pair, execution).map(Self::Lookup),
            _ => parse_operand(pair, execution).map(Self::Value),
        }
    }

    fn operand(&self) -> &Operand {
        match self {
            Self::Lookup(call) => &call.document,
            Self::Value(value) => value,
        }
    }

    fn value<'a>(
        &'a self,
        binding: &'a Binding,
        memory: &mut Reservation,
    ) -> Result<Option<Cow<'a, LiteralValue>>, DatabaseError> {
        match self {
            Self::Lookup(call) => Ok(call.value(binding, memory)?.map(Cow::Owned)),
            Self::Value(value) => Ok(Some(Cow::Borrowed(literal(value, binding)?))),
        }
    }
}

#[derive(Debug, Clone)]
enum Operation {
    Exists {
        call: Call,
        negated: bool,
    },
    Compare {
        left: Expr,
        operator: String,
        right: Expr,
    },
}

#[derive(Debug, Clone)]
pub(super) struct Test {
    pub source: String,
    op: Operation,
}

impl Test {
    pub(super) fn parse(
        pair: Pair<'_, Rule>,
        execution: &ExecutionContext,
    ) -> Result<Self, DatabaseError> {
        let source = pair.as_str().to_owned();
        let mut inner = pair.into_inner();
        let first = inner.next().unwrap();
        let op = if first.as_rule() == Rule::json_comparison {
            let mut parts = first.into_inner();
            Operation::Compare {
                left: Expr::parse(parts.next().unwrap(), execution)?,
                operator: parts.next().unwrap().as_str().to_owned(),
                right: Expr::parse(parts.next().unwrap(), execution)?,
            }
        } else {
            let negated = first.as_rule() == Rule::json_negation;
            let call = Call::parse(
                if negated {
                    inner.next().unwrap()
                } else {
                    first
                },
                execution,
            )?;
            Operation::Exists { call, negated }
        };
        Ok(Self { source, op })
    }

    fn operands(&self) -> impl Iterator<Item = &Operand> {
        let (first, second) = match &self.op {
            Operation::Exists { call, .. } => (&call.document, None),
            Operation::Compare { left, right, .. } => (left.operand(), Some(right.operand())),
        };
        std::iter::once(first).chain(second)
    }

    pub(super) fn variables(&self) -> Vec<&str> {
        self.operands()
            .filter_map(|op| match op {
                Operand::Variable(name) => Some(name.as_str()),
                _ => None,
            })
            .collect()
    }

    pub(super) fn validate(&self, domains: &HashMap<String, Domain>) -> Result<(), DatabaseError> {
        self.operands()
            .try_for_each(|op| validate_literal(op, domains))
    }

    pub(super) fn is_total(&self) -> bool {
        match &self.op {
            Operation::Exists { .. } => true,
            Operation::Compare { operator, .. } => matches!(operator.as_str(), "=" | "?=" | "==="),
        }
    }

    pub(super) fn evaluate(
        &self,
        binding: &Binding,
        execution: &ExecutionContext,
    ) -> Result<bool, DatabaseError> {
        match &self.op {
            Operation::Exists { call, negated } => Ok(call.exists(binding)? != *negated),
            Operation::Compare {
                left,
                operator,
                right,
            } => {
                let mut memory = execution.workspace(|| 0)?;
                let Some(left) = left.value(binding, &mut memory)? else {
                    return Ok(false);
                };
                let Some(right) = right.value(binding, &mut memory)? else {
                    return Ok(false);
                };
                // JSON equality distinguishes strings, numbers and other JSON types.
                // Different literal families can share a nominal domain only for numerics.
                if matches!(operator.as_str(), "=" | "?=")
                    && left.family().nominal_mask() != right.family().nominal_mask()
                {
                    return Ok(false);
                }
                super::compare_literals(&left, operator, &right)
            }
        }
    }
}

pub(super) fn bind_projections(
    binding: &mut Binding,
    columns: &[(String, Call)],
    execution: &ExecutionContext,
) -> Result<Reservation, DatabaseError> {
    if columns.is_empty() {
        return Ok(Reservation::default());
    }
    let mut memory = execution.workspace(|| 0)?;
    for (name, call) in columns {
        execution.work(1)?;
        memory.grow(name.len() as u64 + size_of::<(String, BoundValue)>() as u64 + 48)?;
        let value = call.value(binding, &mut memory)?.ok_or_else(|| DatabaseError::Comparison(format!(
            "{} has no value to return for this row. Use json_exists to select rows containing the field", call.source
        )))?;
        binding.0.insert(name.clone(), BoundValue::Literal(value));
    }
    Ok(memory)
}
