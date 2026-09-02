//! Local Bayesian Spam Filter & Statistical Classifier §7 Phase 6
use ahash::AHashMap;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpamScore {
    pub probability: f64,
    pub is_spam: bool,
    pub significant_tokens: Vec<(String, f64)>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct BayesClassifier {
    pub total_spam_messages: usize,
    pub total_ham_messages: usize,
    pub spam_token_counts: AHashMap<String, usize>,
    pub ham_token_counts: AHashMap<String, usize>,
}

impl BayesClassifier {
    pub fn new() -> Self {
        Self::default()
    }

    /// Tokenize input text into lowercase words with fast byte scanning
    pub fn tokenize(text: &str) -> Vec<String> {
        let mut tokens = Vec::new();
        let bytes = text.as_bytes();
        let mut i = 0;
        while i < bytes.len() {
            while i < bytes.len() {
                let b = bytes[i];
                if b.is_ascii_alphanumeric() || b == b'$' || b == b'!' || b == b'%' {
                    break;
                }
                i += 1;
            }
            let start = i;
            while i < bytes.len() {
                let b = bytes[i];
                if !b.is_ascii_alphanumeric() && b != b'$' && b != b'!' && b != b'%' {
                    break;
                }
                i += 1;
            }
            let len = i - start;
            if (3..=30).contains(&len) {
                let token_slice = &bytes[start..i];
                if simdutf8::basic::from_utf8(token_slice).is_ok() {
                    let mut lower = String::with_capacity(token_slice.len());
                    for &b in token_slice {
                        lower.push(b.to_ascii_lowercase() as char);
                    }
                    tokens.push(lower);
                }
            }
        }
        tokens
    }

    /// Train the classifier with a known spam message
    pub fn train_spam(&mut self, text: &str) {
        self.total_spam_messages += 1;
        let tokens = Self::tokenize(text);
        for token in tokens {
            *self.spam_token_counts.entry(token).or_insert(0) += 1;
        }
    }

    /// Train the classifier with a known legitimate (ham) message
    pub fn train_ham(&mut self, text: &str) {
        self.total_ham_messages += 1;
        let tokens = Self::tokenize(text);
        for token in tokens {
            *self.ham_token_counts.entry(token).or_insert(0) += 1;
        }
    }

    /// Classify a message using Naive Bayes with Laplace smoothing
    pub fn classify(&self, text: &str) -> SpamScore {
        let tokens = Self::tokenize(text);
        if tokens.is_empty() || self.total_spam_messages == 0 || self.total_ham_messages == 0 {
            return SpamScore {
                probability: 0.0,
                is_spam: false,
                significant_tokens: Vec::new(),
            };
        }

        let total_spam = self.total_spam_messages as f64;
        let total_ham = self.total_ham_messages as f64;

        let mut token_probs: Vec<(String, f64)> = Vec::new();

        for token in &tokens {
            let spam_count = self.spam_token_counts.get(token).copied().unwrap_or(0) as f64;
            let ham_count = self.ham_token_counts.get(token).copied().unwrap_or(0) as f64;

            if spam_count + ham_count < 1.0 {
                continue;
            }

            // Frequency calculation with basic prior
            let p_spam = (spam_count / total_spam).min(1.0);
            let p_ham = (ham_count / total_ham).min(1.0);

            if p_spam + p_ham > 0.0 {
                let prob = (p_spam) / (p_spam + p_ham);
                // Clamp to [0.01, 0.99] to avoid extreme zero probabilities
                let clamped = prob.clamp(0.01, 0.99);
                token_probs.push((token.clone(), clamped));
            }
        }

        if token_probs.is_empty() {
            return SpamScore {
                probability: 0.0,
                is_spam: false,
                significant_tokens: Vec::new(),
            };
        }

        // Select top 15 most informative tokens by deviance from 0.5 using O(n) select_nth_unstable + sort
        let top_count = 15.min(token_probs.len());
        let top_tokens: Vec<(String, f64)> = if token_probs.len() > top_count {
            token_probs.select_nth_unstable_by(top_count - 1, |a, b| {
                let dev_a = (a.1 - 0.5).abs();
                let dev_b = (b.1 - 0.5).abs();
                dev_b
                    .partial_cmp(&dev_a)
                    .unwrap_or(std::cmp::Ordering::Equal)
            });
            let mut top = token_probs.into_iter().take(top_count).collect::<Vec<_>>();
            top.sort_by(|a, b| {
                let dev_a = (a.1 - 0.5).abs();
                let dev_b = (b.1 - 0.5).abs();
                dev_b
                    .partial_cmp(&dev_a)
                    .unwrap_or(std::cmp::Ordering::Equal)
            });
            top
        } else {
            let mut top = token_probs;
            top.sort_by(|a, b| {
                let dev_a = (a.1 - 0.5).abs();
                let dev_b = (b.1 - 0.5).abs();
                dev_b
                    .partial_cmp(&dev_a)
                    .unwrap_or(std::cmp::Ordering::Equal)
            });
            top
        };

        // Combine probabilities using Graham-Robinson product formula
        let mut p_prod: f64 = 1.0;
        let mut q_prod: f64 = 1.0;

        for (_, p) in &top_tokens {
            p_prod *= p;
            q_prod *= 1.0 - p;
        }

        let final_prob = if p_prod + q_prod == 0.0 {
            0.5
        } else {
            p_prod / (p_prod + q_prod)
        };

        SpamScore {
            probability: final_prob,
            is_spam: final_prob >= 0.85,
            significant_tokens: top_tokens,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bayes_training_and_classification() {
        let mut bayes = BayesClassifier::new();

        // Train spam
        bayes.train_spam("buy cheap pharmacy pills winner lottery cash free money");
        bayes.train_spam("urgent wire money transfer lottery claim free prize");
        bayes.train_spam("viagra cheap meds casino jackpot online bonus");

        // Train ham
        bayes.train_ham("project rust compiler pull request review weekly standup");
        bayes.train_ham("meeting agenda for tomorrow architecture and performance");
        bayes.train_ham("quarterly engineering roadmap sprint planning notes");

        let spam_test = bayes.classify("claim your free prize wire money now");
        assert!(spam_test.is_spam);
        assert!(spam_test.probability > 0.85);

        let ham_test = bayes.classify("review the pull request for the rust compiler");
        assert!(!ham_test.is_spam);
        assert!(ham_test.probability < 0.20);
    }
}
