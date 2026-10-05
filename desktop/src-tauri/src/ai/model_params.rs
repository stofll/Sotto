//! Request parameters that models of one API accept differently, and the
//! recovery when a model refuses one.
//!
//! Reasoning controls are the case in point: OpenAI's `reasoning_effort`
//! accepts `none` on some models and only `low` on others, Gemini 2.5 Flash
//! turns thinking off with a zero budget while 2.5 Pro answers 400 to it, and
//! new model names keep arriving. A table of names would always lag. Instead a
//! request carries an ordered list of values for each such parameter; a 400
//! that names the parameter moves it to the next value, and past the last one
//! the parameter is left out. The value a model accepted is remembered for the
//! rest of the session, so the refusal costs one fast request once.

use std::collections::HashMap;
use std::future::Future;
use std::sync::{LazyLock, Mutex};

use serde::Serialize;
use serde_json::Value;

use super::providers::{completion_budget, ProviderError, ProviderInfo};

/// How much a reasoning model may think before it answers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ReasoningMode {
    /// As little as the model allows. Tidying a dictation gains nothing from
    /// thinking, and thinking costs time and the answer's token budget.
    #[default]
    Minimal,
    /// Whatever the model does by default; nothing is sent.
    Model,
}

impl ReasoningMode {
    pub fn parse(value: Option<&str>) -> Self {
        match value {
            Some("model") => Self::Model,
            _ => Self::Minimal,
        }
    }
}

/// Smallest explicit limit a profile may set: below it even a short answer
/// with a little thinking does not fit.
pub const MIN_CUSTOM_OUTPUT_TOKENS: u32 = 256;

/// The cap on one answer, thinking included.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum OutputLimit {
    /// Sized to the text by [`completion_budget`].
    #[default]
    Auto,
    /// No cap where the API allows leaving it out.
    Unlimited,
    Tokens(u32),
}

impl OutputLimit {
    /// `llm_output_limit`: `"unlimited"`, a token count, or anything else
    /// for automatic.
    pub fn parse(value: Option<&Value>) -> Self {
        match value {
            Some(Value::String(text)) if text == "unlimited" => Self::Unlimited,
            Some(value) => value
                .as_u64()
                .filter(|tokens| *tokens >= u64::from(MIN_CUSTOM_OUTPUT_TOKENS))
                .map(|tokens| Self::Tokens(u32::try_from(tokens).unwrap_or(u32::MAX)))
                .unwrap_or(Self::Auto),
            None => Self::Auto,
        }
    }

    /// The same cap whatever the text, so a refused value need not be retried.
    pub fn is_fixed(self) -> bool {
        !matches!(self, Self::Auto)
    }

    /// The cap to send for `text`, or `None` to send none.
    pub fn tokens(self, text: &str) -> Option<u32> {
        match self {
            Self::Auto => Some(completion_budget(text)),
            Self::Unlimited => None,
            Self::Tokens(tokens) => Some(tokens),
        }
    }
}

/// One parameter a model may refuse, and the values to try in order.
pub struct Fallback {
    /// Where the parameter lives in the request body.
    pub path: &'static [&'static str],
    /// Words a 400 response uses when it refuses this parameter, lowercase.
    pub keywords: &'static [&'static str],
    /// Values to try in order; after the last one the parameter is removed.
    pub values: Vec<Value>,
    /// Remember the accepted value for the model. Off for values computed
    /// from the text, such as a token limit.
    pub remember: bool,
}

impl Fallback {
    pub fn new(
        path: &'static [&'static str],
        keywords: &'static [&'static str],
        values: Vec<Value>,
    ) -> Self {
        Self {
            path,
            keywords,
            values,
            remember: true,
        }
    }

    /// A token limit: `limit`, then smaller standard caps a model may hold to.
    /// Remembered only when `fixed`: a limit sized to each text would replay
    /// a position computed for another length.
    pub fn limit(
        path: &'static [&'static str],
        keywords: &'static [&'static str],
        limit: u32,
        fixed: bool,
    ) -> Self {
        let mut values = vec![Value::from(limit)];
        values.extend(
            [16_384_u32, 8_192, 4_096]
                .into_iter()
                .filter(|smaller| *smaller < limit)
                .map(Value::from),
        );
        Self {
            path,
            keywords,
            values,
            remember: fixed,
        }
    }
}

/// Accepted value index per model and parameter, for this session.
static ACCEPTED: LazyLock<Mutex<HashMap<String, usize>>> = LazyLock::new(Default::default);

fn memory_key(model_key: &str, fallback: &Fallback) -> String {
    format!("{model_key}|{}", fallback.path.join("."))
}

/// Set the parameter at `path` to `value`, or remove it when `None`.
fn apply(payload: &mut Value, path: &[&str], value: Option<&Value>) {
    let Some((last, parents)) = path.split_last() else {
        return;
    };
    let mut node = payload;
    for key in parents {
        if !node.get(*key).is_some_and(Value::is_object) {
            if value.is_none() {
                return;
            }
            node[*key] = Value::Object(Default::default());
        }
        node = &mut node[*key];
    }
    match value {
        Some(value) => node[*last] = value.clone(),
        None => {
            if let Some(fields) = node.as_object_mut() {
                fields.remove(*last);
            }
        }
    }
}

/// Send `payload` through `post`, stepping each [`Fallback`] down whenever a
/// 400 names it. `model_key` identifies the model for the session memory.
pub async fn post_with_fallbacks<F, Fut>(
    mut payload: Value,
    fallbacks: &[Fallback],
    model_key: &str,
    post: F,
) -> Result<(String, ProviderInfo), ProviderError>
where
    F: Fn(Value) -> Fut,
    Fut: Future<Output = Result<(String, ProviderInfo), ProviderError>>,
{
    let mut positions: Vec<usize> = {
        let accepted = crate::mutex_recover::lock(&ACCEPTED);
        fallbacks
            .iter()
            .map(|fallback| {
                fallback
                    .remember
                    .then(|| accepted.get(&memory_key(model_key, fallback)).copied())
                    .flatten()
                    .unwrap_or(0)
                    .min(fallback.values.len())
            })
            .collect()
    };
    loop {
        for (fallback, position) in fallbacks.iter().zip(&positions) {
            apply(&mut payload, fallback.path, fallback.values.get(*position));
        }
        let result = post(payload.clone()).await;
        let refused = match &result {
            Err(error) if error.http_status == Some(400) => {
                let body = error
                    .response_snippet
                    .as_deref()
                    .unwrap_or_default()
                    .to_ascii_lowercase();
                fallbacks
                    .iter()
                    .zip(&positions)
                    .position(|(fallback, position)| {
                        *position < fallback.values.len()
                            && fallback
                                .keywords
                                .iter()
                                .any(|keyword| body.contains(keyword))
                    })
            }
            _ => None,
        };
        let Some(index) = refused else {
            if result.is_ok() {
                let mut accepted = crate::mutex_recover::lock(&ACCEPTED);
                for (fallback, position) in fallbacks.iter().zip(&positions) {
                    if fallback.remember {
                        accepted.insert(memory_key(model_key, fallback), *position);
                    }
                }
            }
            return result;
        };
        positions[index] += 1;
        log::info!(
            "Model {model_key} refused {}; retrying with the next value",
            fallbacks[index].path.join(".")
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai::providers::ProviderErrorType;
    use serde_json::json;

    #[test]
    fn output_limit_reads_the_three_forms() {
        assert_eq!(OutputLimit::parse(None), OutputLimit::Auto);
        assert_eq!(OutputLimit::parse(Some(&json!("auto"))), OutputLimit::Auto);
        assert_eq!(
            OutputLimit::parse(Some(&json!("unlimited"))),
            OutputLimit::Unlimited
        );
        assert_eq!(
            OutputLimit::parse(Some(&json!(4000))),
            OutputLimit::Tokens(4000)
        );
        // Below the floor a limit would starve every answer: treated as unset.
        assert_eq!(OutputLimit::parse(Some(&json!(10))), OutputLimit::Auto);
    }

    #[test]
    fn reasoning_defaults_to_minimal() {
        assert_eq!(ReasoningMode::parse(None), ReasoningMode::Minimal);
        assert_eq!(
            ReasoningMode::parse(Some("minimal")),
            ReasoningMode::Minimal
        );
        assert_eq!(ReasoningMode::parse(Some("model")), ReasoningMode::Model);
    }

    #[test]
    fn apply_sets_and_removes_nested_fields() {
        let mut payload = json!({"generationConfig": {"temperature": 0.2}});
        let path = &["generationConfig", "thinkingConfig"];
        apply(&mut payload, path, Some(&json!({"thinkingBudget": 0})));
        assert_eq!(
            payload["generationConfig"]["thinkingConfig"]["thinkingBudget"],
            0
        );
        apply(&mut payload, path, None);
        assert!(payload["generationConfig"].get("thinkingConfig").is_none());
        assert_eq!(payload["generationConfig"]["temperature"], 0.2);
        // Removing under a missing parent creates nothing.
        apply(&mut payload, &["absent", "field"], None);
        assert!(payload.get("absent").is_none());
    }

    #[test]
    fn a_limit_steps_down_through_smaller_standard_caps() {
        let values: Vec<u64> = Fallback::limit(&["max_tokens"], &["max_tokens"], 20_000, false)
            .values
            .iter()
            .filter_map(Value::as_u64)
            .collect();
        assert_eq!(values, [20_000, 16_384, 8_192, 4_096]);
        assert_eq!(
            Fallback::limit(&["max_tokens"], &["max_tokens"], 2_048, false)
                .values
                .len(),
            1
        );
    }

    fn refusal(body: &str) -> ProviderError {
        ProviderError {
            kind: ProviderErrorType::ProviderFailed,
            message: "HTTP 400".to_string(),
            http_status: Some(400),
            response_snippet: Some(body.to_string()),
            retryable: false,
        }
    }

    fn ok() -> Result<(String, ProviderInfo), ProviderError> {
        Ok((
            "answer".to_string(),
            ProviderInfo::success("answer", None, 0.1),
        ))
    }

    /// Plays the provider: refuses any payload whose `effort` is in `refused`.
    async fn run(
        model_key: &str,
        refused: &[&str],
        sent: &Mutex<Vec<Value>>,
    ) -> Result<(String, ProviderInfo), ProviderError> {
        let fallbacks = [Fallback::new(
            &["effort"],
            &["effort"],
            vec![json!("none"), json!("minimal"), json!("low")],
        )];
        post_with_fallbacks(json!({}), &fallbacks, model_key, |payload| {
            sent.lock().unwrap().push(payload.clone());
            let effort = payload
                .get("effort")
                .and_then(Value::as_str)
                .unwrap_or("absent")
                .to_string();
            let refuse = refused.contains(&effort.as_str());
            async move {
                if refuse {
                    Err(refusal(&format!(
                        "Unsupported value: 'effort' does not support '{effort}'"
                    )))
                } else {
                    ok()
                }
            }
        })
        .await
    }

    #[tokio::test]
    async fn a_refused_value_steps_to_the_next_and_is_remembered() {
        let sent = Mutex::new(Vec::new());
        run("test/stepping", &["none", "minimal"], &sent)
            .await
            .unwrap();
        let efforts: Vec<Value> = sent
            .lock()
            .unwrap()
            .iter()
            .map(|p| p["effort"].clone())
            .collect();
        assert_eq!(efforts, [json!("none"), json!("minimal"), json!("low")]);

        // The next request starts from the value that worked.
        sent.lock().unwrap().clear();
        run("test/stepping", &["none", "minimal"], &sent)
            .await
            .unwrap();
        assert_eq!(sent.lock().unwrap().len(), 1);
        assert_eq!(sent.lock().unwrap()[0]["effort"], "low");
    }

    #[tokio::test]
    async fn past_the_last_value_the_parameter_is_left_out() {
        let sent = Mutex::new(Vec::new());
        run("test/omitting", &["none", "minimal", "low"], &sent)
            .await
            .unwrap();
        assert!(sent.lock().unwrap().last().unwrap().get("effort").is_none());
    }

    #[tokio::test]
    async fn an_unrelated_400_is_returned_as_is() {
        let fallbacks = [Fallback::new(&["effort"], &["effort"], vec![json!("none")])];
        let calls = Mutex::new(0);
        let result = post_with_fallbacks(json!({}), &fallbacks, "test/unrelated", |_| {
            *calls.lock().unwrap() += 1;
            async { Err(refusal("Invalid model")) }
        })
        .await;
        assert_eq!(result.unwrap_err().http_status, Some(400));
        assert_eq!(*calls.lock().unwrap(), 1);
    }
}
