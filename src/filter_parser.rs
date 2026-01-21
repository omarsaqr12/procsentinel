//! Advanced filter parser with boolean logic and regular expressions

use crate::process::ProcessInfo;
use regex::Regex;
use std::collections::HashMap;

#[derive(Debug, Clone)]
pub enum FilterExpression {
    // Field comparisons
    FieldEquals { field: String, value: String },
    FieldNotEquals { field: String, value: String },
    FieldRegex { field: String, pattern: String },
    FieldGreaterThan { field: String, value: f64 },
    FieldLessThan { field: String, value: f64 },
    FieldGreaterEqual { field: String, value: f64 },
    FieldLessEqual { field: String, value: f64 },
    // Boolean operators
    And(Box<FilterExpression>, Box<FilterExpression>),
    Or(Box<FilterExpression>, Box<FilterExpression>),
    Not(Box<FilterExpression>),
}

pub struct FilterParser {
    regex_cache: HashMap<String, Regex>,
}

impl FilterParser {
    pub fn new() -> Self {
        Self {
            regex_cache: HashMap::new(),
        }
    }

    /// Parse a filter expression string into a FilterExpression AST
    pub fn parse(&mut self, input: &str) -> Result<FilterExpression, String> {
        let input = input.trim();
        if input.is_empty() {
            return Err("Empty filter expression".to_string());
        }
        
        // Simple recursive descent parser
        self.parse_expression(input)
    }

    fn parse_expression(&mut self, input: &str) -> Result<FilterExpression, String> {
        let input = input.trim();
        
        // Check for NOT operator
        if input.starts_with("NOT ") || input.starts_with("not ") {
            let rest = input[4..].trim();
            if rest.starts_with('(') && rest.ends_with(')') {
                let inner = &rest[1..rest.len()-1];
                let expr = self.parse_expression(inner)?;
                return Ok(FilterExpression::Not(Box::new(expr)));
            } else {
                let expr = self.parse_expression(rest)?;
                return Ok(FilterExpression::Not(Box::new(expr)));
            }
        }
        
        // Check for parentheses
        if input.starts_with('(') && input.ends_with(')') {
            // Try to find matching closing paren
            let mut depth = 0;
            let mut end_pos = 0;
            for (i, c) in input.chars().enumerate() {
                match c {
                    '(' => depth += 1,
                    ')' => {
                        depth -= 1;
                        if depth == 0 {
                            end_pos = i;
                            break;
                        }
                    }
                    _ => {}
                }
            }
            if end_pos == input.len() - 1 {
                // Full expression in parentheses
                return self.parse_expression(&input[1..input.len()-1]);
            }
        }
        
        // Check for AND/OR operators (lower precedence)
        // Split by AND/OR, respecting parentheses
        let and_pos = self.find_operator(input, "AND");
        let or_pos = self.find_operator(input, "OR");
        
        if let Some(pos) = or_pos {
            let left = self.parse_expression(&input[..pos])?;
            let right = self.parse_expression(&input[pos+3..])?;
            return Ok(FilterExpression::Or(Box::new(left), Box::new(right)));
        }
        
        if let Some(pos) = and_pos {
            let left = self.parse_expression(&input[..pos])?;
            let right = self.parse_expression(&input[pos+3..])?;
            return Ok(FilterExpression::And(Box::new(left), Box::new(right)));
        }
        
        // Parse field comparison
        self.parse_comparison(input)
    }

    fn find_operator(&self, input: &str, op: &str) -> Option<usize> {
        let op_upper = op.to_uppercase();
        let op_lower = op.to_lowercase();
        let mut depth = 0;
        
        for (i, _) in input.char_indices() {
            if i + op.len() > input.len() {
                break;
            }
            
            let substr = &input[i..i+op.len()];
            if substr == op_upper || substr == op_lower {
                // Check if it's a word boundary
                let before = if i > 0 { input.chars().nth(i-1) } else { Some(' ') };
                let after = input.chars().nth(i + op.len());
                
                if let (Some(b), Some(a)) = (before, after) {
                    if b.is_whitespace() && a.is_whitespace() && depth == 0 {
                        return Some(i);
                    }
                }
            }
            
            match input.chars().nth(i) {
                Some('(') => depth += 1,
                Some(')') => depth -= 1,
                _ => {}
            }
        }
        None
    }

    fn parse_comparison(&mut self, input: &str) -> Result<FilterExpression, String> {
        let input = input.trim();
        
        // Try different comparison operators (check longer ones first)
        let operators = [">=", "<=", "~=", "==", "!=", ">", "<"];
        
        for op in operators.iter() {
            if let Some(pos) = input.find(op) {
                let field = input[..pos].trim().to_lowercase();
                let value = input[pos + op.len()..].trim();
                
                // Remove quotes if present
                let value = value.trim_matches('"').trim_matches('\'');
                
                // Handle regex operator
                if *op == "~=" || *op == "~" {
                    return Ok(FilterExpression::FieldRegex { 
                        field: field, 
                        pattern: value.to_string() 
                    });
                }
                
                // Handle numeric comparisons
                if *op == ">" || *op == "<" || *op == ">=" || *op == "<=" {
                    let num_value = value.parse::<f64>()
                        .map_err(|_| format!("Invalid number: {}", value))?;
                    return match *op {
                        ">" => Ok(FilterExpression::FieldGreaterThan { field: field, value: num_value }),
                        "<" => Ok(FilterExpression::FieldLessThan { field: field, value: num_value }),
                        ">=" => Ok(FilterExpression::FieldGreaterEqual { field: field, value: num_value }),
                        "<=" => Ok(FilterExpression::FieldLessEqual { field: field, value: num_value }),
                        _ => unreachable!(),
                    };
                }
                
                // Handle string comparisons
                return match *op {
                    "==" => Ok(FilterExpression::FieldEquals { field: field, value: value.to_string() }),
                    "!=" => Ok(FilterExpression::FieldNotEquals { field: field, value: value.to_string() }),
                    _ => Err(format!("Unknown operator: {}", op)),
                };
            }
        }
        
        // Try regex operator ~ (without =)
        if let Some(pos) = input.find('~') {
            let field = input[..pos].trim().to_lowercase();
            let pattern = input[pos + 1..].trim().trim_matches('"').trim_matches('\'');
            return Ok(FilterExpression::FieldRegex { 
                field: field.to_string(), 
                pattern: pattern.to_string() 
            });
        }
        
        Err(format!("Invalid filter expression: {}", input))
    }

    /// Evaluate a filter expression against a process
    pub fn evaluate(&mut self, process: &ProcessInfo, expr: &FilterExpression) -> bool {
        match expr {
            FilterExpression::FieldEquals { field, value } => {
                self.get_field_value(process, field) == *value
            }
            FilterExpression::FieldNotEquals { field, value } => {
                self.get_field_value(process, field) != *value
            }
            FilterExpression::FieldRegex { field, pattern } => {
                let field_value = self.get_field_value(process, field);
                // Get or compile regex
                let regex = self.regex_cache.entry(pattern.clone())
                    .or_insert_with(|| Regex::new(pattern).unwrap_or_else(|_| Regex::new("^$").unwrap()));
                regex.is_match(&field_value)
            }
            FilterExpression::FieldGreaterThan { field, value } => {
                self.get_numeric_field(process, field) > *value
            }
            FilterExpression::FieldLessThan { field, value } => {
                self.get_numeric_field(process, field) < *value
            }
            FilterExpression::FieldGreaterEqual { field, value } => {
                self.get_numeric_field(process, field) >= *value
            }
            FilterExpression::FieldLessEqual { field, value } => {
                self.get_numeric_field(process, field) <= *value
            }
            FilterExpression::And(left, right) => {
                self.evaluate(process, left) && self.evaluate(process, right)
            }
            FilterExpression::Or(left, right) => {
                self.evaluate(process, left) || self.evaluate(process, right)
            }
            FilterExpression::Not(expr) => {
                !self.evaluate(process, expr)
            }
        }
    }

    fn get_field_value(&self, process: &ProcessInfo, field: &str) -> String {
        match field {
            "name" => process.name.clone(),
            "user" => process.user.clone().unwrap_or_default(),
            "status" => process.status.clone(),
            // Handle numeric fields as strings for equality checks
            "pid" => process.pid.to_string(),
            "ppid" => process.parent_pid.unwrap_or(0).to_string(),
            "nice" => process.nice.to_string(),
            "cpu" => format!("{:.1}", process.cpu_usage),
            "memory" => format!("{}", process.memory_usage / (1024 * 1024)),
            _ => String::new(),
        }
    }

    fn get_numeric_field(&self, process: &ProcessInfo, field: &str) -> f64 {
        match field {
            "pid" => process.pid as f64,
            "ppid" => process.parent_pid.unwrap_or(0) as f64,
            "cpu" => process.cpu_usage as f64,
            "memory" => (process.memory_usage / (1024 * 1024)) as f64, // MB
            "nice" => process.nice as f64,
            _ => 0.0,
        }
    }
}

impl Default for FilterParser {
    fn default() -> Self {
        Self::new()
    }
}

