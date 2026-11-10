//! Sandbox escrow for the jury contract.
//!
//! A deal holds NEAR for one jury case. The recipients and the amount are
//! fixed when the deal is funded. `settle` reads the jury outcome and pays
//! only on a `Decided` outcome for the worker option or the requester option.
//! Every other outcome leaves the funds held. `settle` is safe to call again.
//!
//! This contract is a demonstration. It has no connection to the payment
//! system of NEAR Agent Market, and it has no way to release held funds.

use near_sdk::serde::Deserialize;
use near_sdk::serde_json::json;
use near_sdk::store::LookupMap;
use near_sdk::{env, near, require, AccountId, Gas, NearToken, PanicOnDefault, Promise, PromiseError};

const VIEW_GAS: Gas = Gas::from_tgas(15);
const ON_CASE_GAS: Gas = Gas::from_tgas(40);
const ON_PAID_GAS: Gas = Gas::from_tgas(10);

#[near(serializers = [borsh, json])]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    /// The funds are in this contract. `settle` can run.
    Held,
    /// A `settle` call is in progress. A second call is refused.
    Settling,
    PaidWorker,
    RefundedRequester,
}

#[near(serializers = [borsh, json])]
#[derive(Clone, Debug)]
pub struct Deal {
    pub jury_case_id: u64,
    pub funder: AccountId,
    pub worker: AccountId,
    pub requester: AccountId,
    pub worker_option: u32,
    pub requester_option: u32,
    pub amount: NearToken,
    pub status: Status,
    /// The jury outcome that the last `settle` call read.
    pub last_jury_outcome: Option<String>,
    pub last_jury_ruling: Option<u32>,
    pub settle_calls: u32,
    pub failed_transfers: u32,
}

/// The fields of the jury `get_case` view that the escrow uses.
#[derive(Deserialize)]
#[serde(crate = "near_sdk::serde")]
pub struct JuryCase {
    pub outcome: String,
    pub ruling: Option<u32>,
}

#[near(contract_state)]
#[derive(PanicOnDefault)]
pub struct Escrow {
    jury: AccountId,
    deals: LookupMap<u64, Deal>,
    next_deal_id: u64,
}

#[near]
impl Escrow {
    #[init]
    pub fn new(jury: AccountId) -> Self {
        Self { jury, deals: LookupMap::new(b"d"), next_deal_id: 0 }
    }

    /// Fund a deal for one jury case. The attached NEAR is the payment.
    #[payable]
    pub fn fund(&mut self, jury_case_id: u64, worker: AccountId, requester: AccountId, worker_option: u32, requester_option: u32) -> u64 {
        let amount = env::attached_deposit();
        require!(amount > NearToken::from_yoctonear(0), "attach the payment");
        require!(worker_option != requester_option, "the two options must differ");
        require!(worker != requester, "the two recipients must differ");
        let id = self.next_deal_id;
        self.next_deal_id += 1;
        self.deals.insert(
            id,
            Deal {
                jury_case_id,
                funder: env::predecessor_account_id(),
                worker,
                requester,
                worker_option,
                requester_option,
                amount,
                status: Status::Held,
                last_jury_outcome: None,
                last_jury_ruling: None,
                settle_calls: 0,
                failed_transfers: 0,
            },
        );
        id
    }

    /// Read the jury outcome and pay if it is decided for one party.
    /// Anyone can call this. A deal that is paid cannot be paid again.
    pub fn settle(&mut self, deal_id: u64) -> Promise {
        let deal = self.deals.get_mut(&deal_id).expect("no such deal");
        require!(deal.status == Status::Held, "the deal is not held: it is paid or a settle call is in progress");
        deal.status = Status::Settling;
        deal.settle_calls += 1;
        let args = json!({ "case_id": deal.jury_case_id }).to_string().into_bytes();
        Promise::new(self.jury.clone())
            .function_call("get_case".to_string(), args, NearToken::from_yoctonear(0), VIEW_GAS)
            .then(Self::ext(env::current_account_id()).with_static_gas(ON_CASE_GAS).on_case(deal_id))
    }

    #[private]
    pub fn on_case(&mut self, deal_id: u64, #[callback_result] case: Result<JuryCase, PromiseError>) -> String {
        let deal = self.deals.get_mut(&deal_id).expect("no such deal");
        let Ok(case) = case else {
            deal.status = Status::Held;
            return "held: the jury contract did not answer".to_string();
        };
        deal.last_jury_outcome = Some(case.outcome.clone());
        deal.last_jury_ruling = case.ruling;
        let pay_to = match (case.outcome.as_str(), case.ruling) {
            ("Decided", Some(r)) if r == deal.worker_option => Some((deal.worker.clone(), Status::PaidWorker)),
            ("Decided", Some(r)) if r == deal.requester_option => Some((deal.requester.clone(), Status::RefundedRequester)),
            _ => None,
        };
        match pay_to {
            None => {
                deal.status = Status::Held;
                format!("held: jury outcome is {}", case.outcome)
            }
            Some((to, paid_status)) => {
                // The status stays `Settling` until the transfer result arrives.
                let _ = Promise::new(to)
                    .transfer(deal.amount)
                    .then(Self::ext(env::current_account_id()).with_static_gas(ON_PAID_GAS).on_paid(deal_id, paid_status));
                "transfer started".to_string()
            }
        }
    }

    #[private]
    pub fn on_paid(&mut self, deal_id: u64, paid_status: Status, #[callback_result] transfer: Result<(), PromiseError>) {
        let deal = self.deals.get_mut(&deal_id).expect("no such deal");
        if transfer.is_ok() {
            deal.status = paid_status;
        } else {
            // A failed transfer returns the NEAR to this contract. Settle can run again.
            deal.status = Status::Held;
            deal.failed_transfers += 1;
        }
    }

    pub fn get_deal(&self, deal_id: u64) -> Deal {
        self.deals.get(&deal_id).expect("no such deal").clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use near_sdk::test_utils::VMContextBuilder;
    use near_sdk::testing_env;

    fn acct(s: &str) -> AccountId {
        s.parse().unwrap()
    }

    fn funded() -> Escrow {
        testing_env!(VMContextBuilder::new()
            .predecessor_account_id(acct("buyer.near"))
            .attached_deposit(NearToken::from_near(2))
            .build());
        let mut e = Escrow::new(acct("jury.near"));
        e.fund(0, acct("worker.near"), acct("buyer.near"), 0, 2);
        e
    }

    #[test]
    fn fund_binds_recipients_and_amount() {
        let e = funded();
        let d = e.get_deal(0);
        assert_eq!((d.status, d.amount), (Status::Held, NearToken::from_near(2)));
        assert_eq!((d.worker, d.requester), (acct("worker.near"), acct("buyer.near")));
    }

    #[test]
    #[should_panic(expected = "the deal is not held")]
    fn second_settle_while_first_is_in_progress_fails() {
        let mut e = funded();
        let _ = e.settle(0);
        let _ = e.settle(0);
    }

    #[test]
    #[should_panic(expected = "attach the payment")]
    fn fund_without_payment_fails() {
        testing_env!(VMContextBuilder::new().predecessor_account_id(acct("buyer.near")).build());
        let mut e = Escrow::new(acct("jury.near"));
        e.fund(0, acct("worker.near"), acct("buyer.near"), 0, 2);
    }
}
