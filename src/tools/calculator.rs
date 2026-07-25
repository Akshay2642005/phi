//! Tool skill — Calculator
//!
//! Evaluates basic arithmetic expressions containing +, -, *, / and
//! parentheses.  Uses a hand-written recursive-descent parser — no
//! `eval()` or external dependency.
//!
//! Slot read:    `ctx.input.goal`  — goal containing the expression
//! Slot written: `"calculator"`    → [`CalculatorOutput`]
//! Dependencies: none

use std::sync::Arc;

use anyhow::{Result, anyhow};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::core::{
    context::WorkflowContext,
    skill::{Skill, SkillDefinition},
};
use crate::runtime::RuntimeInput;

/// Output produced by the calculator skill.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CalculatorOutput {
    pub expression: String,
    pub result: f64,
}

pub const DEFINITION: SkillDefinition = SkillDefinition {
    id: "calculator",
    name: "Calculator",
    description: "Evaluates basic arithmetic expressions (+, -, *, /).",
    version: "1.0.0",
    dependencies: &[],
};

pub struct CalculatorSkill;

impl CalculatorSkill {
    pub fn new() -> Arc<Self> {
        Arc::new(Self)
    }
}

/// Extract an arithmetic expression from the goal string.
fn extract_expression(goal: &str) -> Result<String> {
    let trimmed = goal.trim();

    // Remove common prefixes.
    let prefixes = [
        "calculate ",
        "calc ",
        "compute ",
        "math ",
        "evaluate ",
        "what is ",
        "what's ",
    ];
    for prefix in &prefixes {
        if let Some(stripped) = trimmed.strip_prefix(prefix) {
            let rest = stripped.trim();
            if !rest.is_empty() {
                return Ok(rest.to_string());
            }
        }
    }

    if trimmed.contains(|c: char| c.is_ascii_digit() || "+-*/()".contains(c)) {
        Ok(trimmed.to_string())
    } else {
        Err(anyhow!(
            "could not extract an arithmetic expression from goal: '{}'",
            goal
        ))
    }
}

#[async_trait]
impl Skill<RuntimeInput> for CalculatorSkill {
    fn definition(&self) -> &'static SkillDefinition {
        &DEFINITION
    }

    async fn execute(&self, ctx: &mut WorkflowContext<RuntimeInput>) -> Result<()> {
        let goal = &ctx.input.goal;
        let expression = extract_expression(goal)?;

        tracing::info!(
            skill = DEFINITION.id,
            expression = %expression,
            "Evaluating expression"
        );

        let result = eval_arithmetic(&expression)?;

        tracing::info!(
            skill = DEFINITION.id,
            expression = %expression,
            result,
            "Calculation complete"
        );

        ctx.set(DEFINITION.id, CalculatorOutput { expression, result });

        Ok(())
    }
}

// ── Recursive-descent arithmetic parser ─────────────────────────────────

/// Token types for the expression parser.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Token {
    Number(f64),
    Plus,
    Minus,
    Star,
    Slash,
    LParen,
    RParen,
    End,
}

/// Tokenizer — converts an expression string into tokens.
struct Lexer {
    chars: Vec<char>,
    pos: usize,
}

impl Lexer {
    fn new(input: &str) -> Self {
        Self {
            chars: input.chars().collect(),
            pos: 0,
        }
    }

    fn skip_whitespace(&mut self) {
        while self.pos < self.chars.len() && self.chars[self.pos].is_whitespace() {
            self.pos += 1;
        }
    }

    fn next_token(&mut self) -> Result<Token> {
        self.skip_whitespace();
        if self.pos >= self.chars.len() {
            return Ok(Token::End);
        }

        let c = self.chars[self.pos];
        match c {
            '+' => {
                self.pos += 1;
                Ok(Token::Plus)
            }
            '-' => {
                self.pos += 1;
                Ok(Token::Minus)
            }
            '*' => {
                self.pos += 1;
                Ok(Token::Star)
            }
            '/' => {
                self.pos += 1;
                Ok(Token::Slash)
            }
            '(' => {
                self.pos += 1;
                Ok(Token::LParen)
            }
            ')' => {
                self.pos += 1;
                Ok(Token::RParen)
            }
            c if c.is_ascii_digit() || c == '.' => {
                let start = self.pos;
                while self.pos < self.chars.len()
                    && (self.chars[self.pos].is_ascii_digit() || self.chars[self.pos] == '.')
                {
                    self.pos += 1;
                }
                let s: String = self.chars[start..self.pos].iter().collect();
                let n: f64 = s.parse().map_err(|_| anyhow!("invalid number: '{}'", s))?;
                Ok(Token::Number(n))
            }
            _ => Err(anyhow!("unexpected character '{}' in expression", c)),
        }
    }
}

/// Evaluate an arithmetic expression string.
pub fn eval_arithmetic(expr: &str) -> Result<f64> {
    if expr.trim().is_empty() {
        return Err(anyhow!("empty expression"));
    }

    let mut lexer = Lexer::new(expr);
    let mut current_token = lexer.next_token()?;
    let result = parse_add_sub(&mut lexer, &mut current_token)?;

    if current_token != Token::End {
        return Err(anyhow!("unexpected token after expression"));
    }

    Ok(result)
}

/// Parse addition and subtraction.
fn parse_add_sub(lexer: &mut Lexer, token: &mut Token) -> Result<f64> {
    let mut left = parse_mul_div(lexer, token)?;

    while *token == Token::Plus || *token == Token::Minus {
        let op = *token;
        *token = lexer.next_token()?;
        let right = parse_mul_div(lexer, token)?;
        match op {
            Token::Plus => left += right,
            Token::Minus => left -= right,
            _ => unreachable!(),
        }
    }

    Ok(left)
}

/// Parse multiplication and division.
fn parse_mul_div(lexer: &mut Lexer, token: &mut Token) -> Result<f64> {
    let mut left = parse_primary(lexer, token)?;

    while *token == Token::Star || *token == Token::Slash {
        let op = *token;
        *token = lexer.next_token()?;
        let right = parse_primary(lexer, token)?;
        match op {
            Token::Star => left *= right,
            Token::Slash => {
                if right == 0.0 {
                    return Err(anyhow!("division by zero"));
                }
                left /= right;
            }
            _ => unreachable!(),
        }
    }

    Ok(left)
}

/// Parse primary expressions (numbers and parenthesized expressions).
fn parse_primary(lexer: &mut Lexer, token: &mut Token) -> Result<f64> {
    match *token {
        Token::Number(n) => {
            *token = lexer.next_token()?;
            Ok(n)
        }
        Token::LParen => {
            *token = lexer.next_token()?;
            let value = parse_add_sub(lexer, token)?;
            if *token != Token::RParen {
                return Err(anyhow!("expected closing parenthesis"));
            }
            *token = lexer.next_token()?;
            Ok(value)
        }
        Token::Minus => {
            // Unary minus
            *token = lexer.next_token()?;
            let value = parse_primary(lexer, token)?;
            Ok(-value)
        }
        _ => Err(anyhow!("expected number or '('")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::context::WorkflowContext;

    #[tokio::test]
    async fn simple_addition() {
        let skill = CalculatorSkill::new();
        let input = RuntimeInput {
            goal: "calculate 2 + 3".to_string(),
            context: None,
        };
        let (mut ctx, _tx) = WorkflowContext::new(input);

        skill.execute(&mut ctx).await.expect("2 + 3 should succeed");

        let output = ctx
            .get::<CalculatorOutput>(DEFINITION.id)
            .expect("output should be in context");
        assert!((output.result - 5.0).abs() < 1e-10, "2 + 3 = 5");
    }

    #[tokio::test]
    async fn complex_expression() {
        let skill = CalculatorSkill::new();
        let input = RuntimeInput {
            goal: "(2 + 3) * 4 - 6 / 2".to_string(),
            context: None,
        };
        let (mut ctx, _tx) = WorkflowContext::new(input);

        skill
            .execute(&mut ctx)
            .await
            .expect("complex expression should succeed");

        let output = ctx
            .get::<CalculatorOutput>(DEFINITION.id)
            .expect("output should be in context");
        // (2 + 3) * 4 - 6 / 2 = 5 * 4 - 3 = 20 - 3 = 17
        assert!(
            (output.result - 17.0).abs() < 1e-10,
            "expected 17, got {}",
            output.result
        );
    }

    #[tokio::test]
    async fn division_by_zero_errors() {
        let skill = CalculatorSkill::new();
        let input = RuntimeInput {
            goal: "1 / 0".to_string(),
            context: None,
        };
        let (mut ctx, _tx) = WorkflowContext::new(input);

        let result = skill.execute(&mut ctx).await;
        assert!(result.is_err(), "division by zero should fail");
        let err = format!("{}", result.unwrap_err());
        assert!(
            err.contains("division by zero"),
            "error should mention division by zero"
        );
    }

    #[tokio::test]
    async fn unary_minus() {
        let skill = CalculatorSkill::new();
        let input = RuntimeInput {
            goal: "-5 + 3".to_string(),
            context: None,
        };
        let (mut ctx, _tx) = WorkflowContext::new(input);

        skill
            .execute(&mut ctx)
            .await
            .expect("-5 + 3 should succeed");

        let output = ctx
            .get::<CalculatorOutput>(DEFINITION.id)
            .expect("output should be in context");
        assert!(
            (output.result - (-2.0)).abs() < 1e-10,
            "expected -2, got {}",
            output.result
        );
    }

    #[tokio::test]
    async fn nested_parentheses() {
        let skill = CalculatorSkill::new();
        let input = RuntimeInput {
            goal: "((2 + 3) * (4 - 1))".to_string(),
            context: None,
        };
        let (mut ctx, _tx) = WorkflowContext::new(input);

        skill
            .execute(&mut ctx)
            .await
            .expect("nested parens should succeed");

        let output = ctx
            .get::<CalculatorOutput>(DEFINITION.id)
            .expect("output should be in context");
        // ((2 + 3) * (4 - 1)) = 5 * 3 = 15
        assert!(
            (output.result - 15.0).abs() < 1e-10,
            "expected 15, got {}",
            output.result
        );
    }

    #[tokio::test]
    async fn fails_on_empty_goal() {
        let skill = CalculatorSkill::new();
        let input = RuntimeInput {
            goal: "".to_string(),
            context: None,
        };
        let (mut ctx, _tx) = WorkflowContext::new(input);

        let result = skill.execute(&mut ctx).await;
        assert!(result.is_err(), "empty goal should fail");
    }
}
