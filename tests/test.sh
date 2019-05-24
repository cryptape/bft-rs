RUST_BACKTRACE=full cargo test basic_test
RUST_BACKTRACE=full cargo test --features=machine_gun basic_test
RUST_BACKTRACE=full cargo test --features=verify_req basic_test
RUST_BACKTRACE=full cargo test --features=machine_gun,verify_req basic_test
