//! Fault specs and the primitives that apply them. Every primitive acts on a simulated pod, so
//! in the co-located layout one step reaches a voter of each cluster.

use crate::proxy::Rules;
use crate::run::{RunCtx, StopSignal};
use std::fmt;
use std::str::FromStr;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FaultStep {
    SigKill(usize),
    SigTerm(usize),
    Isolate(usize),
    LinkDown(usize, usize),
    Heal,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FaultSpec(pub Vec<FaultStep>);

impl FaultSpec {
    pub fn is_none(&self) -> bool {
        self.0.is_empty()
    }
}

impl FromStr for FaultSpec {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let s = s.trim();
        if s.is_empty() || s == "none" {
            return Ok(Self::default());
        }
        let pod = |v: &str| {
            v.parse::<usize>()
                .map_err(|_| format!("fault: `{v}` is not a pod index"))
        };
        let mut steps = Vec::new();
        for part in s.split(',') {
            let part = part.trim();
            let step = match part.split(':').collect::<Vec<_>>().as_slice() {
                ["sigkill", p] => FaultStep::SigKill(pod(p)?),
                ["sigterm", p] => FaultStep::SigTerm(pod(p)?),
                ["isolate", p] => FaultStep::Isolate(pod(p)?),
                ["link-down", a, b] => {
                    let (a, b) = (pod(a)?, pod(b)?);
                    if a == b {
                        return Err(format!("fault: `{part}` names one pod twice"));
                    }
                    FaultStep::LinkDown(a, b)
                }
                ["heal"] => FaultStep::Heal,
                _ => return Err(format!("fault: cannot parse step `{part}`")),
            };
            steps.push(step);
        }
        Ok(Self(steps))
    }
}

impl fmt::Display for FaultSpec {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.0.is_empty() {
            return f.write_str("none");
        }
        let parts: Vec<String> = self
            .0
            .iter()
            .map(|s| match s {
                FaultStep::SigKill(p) => format!("sigkill:{p}"),
                FaultStep::SigTerm(p) => format!("sigterm:{p}"),
                FaultStep::Isolate(p) => format!("isolate:{p}"),
                FaultStep::LinkDown(a, b) => format!("link-down:{a}:{b}"),
                FaultStep::Heal => "heal".into(),
            })
            .collect();
        f.write_str(&parts.join(","))
    }
}

/// The proxy rules that isolate every node of `pod` from every node outside it.
pub fn isolate_rules(mut rules: Rules, ctx: &RunCtx, pod: usize) -> Rules {
    let inside = &ctx.topo.pods[pod].1;
    for &n in inside {
        for ep in ctx.proxy.endpoints_of(n) {
            rules.down_endpoints.insert(ep);
        }
        for other in ctx.topo.nodes.iter().map(|x| x.key) {
            if !inside.contains(&other) {
                rules.blocked_links.insert((n, other));
            }
        }
    }
    rules
}

pub fn link_down_rules(mut rules: Rules, ctx: &RunCtx, a: usize, b: usize) -> Rules {
    for &x in &ctx.topo.pods[a].1 {
        for &y in &ctx.topo.pods[b].1 {
            rules.blocked_links.insert((x, y));
            rules.blocked_links.insert((y, x));
        }
    }
    rules
}

/// Applies one step. Signal steps update which nodes the run expects alive.
pub async fn apply(ctx: &mut RunCtx, step: &FaultStep) -> Result<(), String> {
    let pods = ctx.topo.pods.len();
    let check = |p: usize| {
        if p < pods {
            Ok(())
        } else {
            Err(format!("fault names pod {p}, the layout has {pods}"))
        }
    };
    match step {
        FaultStep::SigKill(p) | FaultStep::SigTerm(p) => {
            check(*p)?;
            let sig = if matches!(step, FaultStep::SigKill(_)) {
                StopSignal::Kill
            } else {
                StopSignal::Term
            };
            let keys = ctx.topo.pods[*p].1.clone();
            ctx.stop(&keys, sig, &format!("fault {step:?}")).await;
        }
        FaultStep::Isolate(p) => {
            check(*p)?;
            let rules = isolate_rules(ctx.proxy.rules(), ctx, *p);
            ctx.proxy.set_rules(rules);
            ctx.event(format!("proxy: pod {p} isolated"));
        }
        FaultStep::LinkDown(a, b) => {
            check(*a)?;
            check(*b)?;
            let rules = link_down_rules(ctx.proxy.rules(), ctx, *a, *b);
            ctx.proxy.set_rules(rules);
            ctx.event(format!("proxy: link pod {a} <-> pod {b} down"));
        }
        FaultStep::Heal => {
            ctx.proxy.set_rules(Rules::default());
            ctx.event("proxy: healed".into());
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_and_prints() {
        assert!(FaultSpec::from_str("none").unwrap().is_none());
        let f = FaultSpec::from_str("sigkill:0, isolate:2,link-down:0:1,heal").unwrap();
        assert_eq!(
            f.0,
            vec![
                FaultStep::SigKill(0),
                FaultStep::Isolate(2),
                FaultStep::LinkDown(0, 1),
                FaultStep::Heal
            ]
        );
        assert_eq!(f.to_string(), "sigkill:0,isolate:2,link-down:0:1,heal");
        assert!(FaultSpec::from_str("partition").is_err());
        assert!(FaultSpec::from_str("link-down:1:1").is_err());
        assert!(FaultSpec::from_str("sigkill:x").is_err());
    }
}
