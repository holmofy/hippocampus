use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReactError {
    /// 既没有 Final Answer，也解析不出 Action。
    NoActionOrFinal,
    /// 超过 `max_steps`。
    MaxSteps,
}

impl fmt::Display for ReactError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoActionOrFinal => write!(
                f,
                "model output contained neither 'Final Answer:' nor a valid Action/Action Input pair"
            ),
            Self::MaxSteps => write!(f, "exceeded max ReAct steps"),
        }
    }
}

impl std::error::Error for ReactError {}
