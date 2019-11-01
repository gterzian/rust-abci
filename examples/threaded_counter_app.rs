extern crate abci;
extern crate byteorder;
extern crate env_logger;

use abci::*;
use byteorder::{BigEndian, ByteOrder};
use crossbeam_channel::{unbounded, Receiver, Sender};
use env_logger::Env;
use std::thread;

// Simple counter application.  Its only state is a u64 count
// We use BigEndian to serialize the data across transactions calls
struct CounterApp {
    count: u64,
    receiver: Receiver<CounterMsg>,
    exiter: Exiter,
}

// Convert incoming tx data to the proper BigEndian size. txs.len() > 8 will return 0
fn convert_tx(tx: &[u8]) -> u64 {
    if tx.len() < 8 {
        let pad = 8 - tx.len();
        let mut x = vec![0; pad];
        x.extend_from_slice(tx);
        return BigEndian::read_u64(x.as_slice());
    }
    BigEndian::read_u64(tx)
}

enum CounterMsg {
    CheckTx(RequestCheckTx, Responder),
    DeliverTx(RequestDeliverTx, Responder),
    Commit(RequestCommit, Responder),
    Exit,
}

/// The front-end to the application, forwards all requests on a channel.
struct CounterProxy {
    sender: Sender<CounterMsg>,
    receiver: Option<Receiver<CounterMsg>>,
}

impl CounterProxy {
    fn new() -> Self {
        let (sender, receiver) = unbounded();
        CounterProxy {
            sender,
            receiver: Some(receiver),
        }
    }
}

impl abci::Application for CounterProxy {
    fn start(&mut self, exiter: Exiter) {
        let mut app = CounterApp {
            count: 0,
            receiver: self
                .receiver
                .take()
                .expect("CounterProxy to have a receiver"),
            exiter,
        };
        thread::spawn(move || while app.run() {});
    }

    fn exit(&mut self) {
        let _ = self.sender.send(CounterMsg::Exit);
    }

    // Validate transactions.  Rule:  Transactions must be incremental: 1,2,3,4...
    fn check_tx(&mut self, req: RequestCheckTx, responder: Responder) {
        let _ = self.sender.send(CounterMsg::CheckTx(req, responder));
    }

    fn deliver_tx(&mut self, req: RequestDeliverTx, responder: Responder) {
        let _ = self.sender.send(CounterMsg::DeliverTx(req, responder));
    }

    fn commit(&mut self, req: RequestCommit, responder: Responder) {
        let _ = self.sender.send(CounterMsg::Commit(req, responder));
    }
}

impl CounterApp {
    fn run(&mut self) -> bool {
        while let Ok(msg) = self.receiver.recv() {
            match msg {
                CounterMsg::CheckTx(req, responder) => self.check_tx(req, responder),
                CounterMsg::DeliverTx(req, responder) => self.deliver_tx(req, responder),
                CounterMsg::Commit(req, responder) => self.commit(req, responder),
                CounterMsg::Exit => {
                    // The ABCI layer sent us a message to quit.
                    return false;
                }
            }
            // For some reason, stop when the count hits 10,
            // signalling the ABCI layer to quit.
            if self.count > 10 {
                self.exiter.exit();
                return false;
            }
        }
        // Proxy went away, unexpected.
        false
    }

    // Validate transactions.  Rule:  Transactions must be incremental: 1,2,3,4...
    fn check_tx(&mut self, req: RequestCheckTx, mut responder: Responder) {
        // Get the Tx [u8] and convert to u64
        let c = convert_tx(req.get_tx());
        let mut resp = ResponseCheckTx::new();

        // Validation logic
        if c != self.count + 1 {
            resp.set_code(1);
            resp.set_log(String::from("Count must be incremental!"));
        } else {
            // Update state to keep state correct for next check_tx call
            self.count = c;
        }

        let mut response = Response::new();
        response.set_check_tx(resp);
        let _ = responder.respond(response);
    }

    fn deliver_tx(&mut self, req: RequestDeliverTx, mut responder: Responder) {
        // Get the Tx [u8]
        let c = convert_tx(req.get_tx());
        // Update state
        self.count = c;
        // Return default code 0 == bueno
        let res = ResponseDeliverTx::new();
        let mut response = Response::new();
        response.set_deliver_tx(res);
        let _ = responder.respond(response);
    }

    fn commit(&mut self, _req: RequestCommit, mut responder: Responder) {
        // Create the response
        let mut resp = ResponseCommit::new();
        // Convert count to bits
        let mut buf = [0; 8];
        BigEndian::write_u64(&mut buf, self.count);
        // Set data so last state is included in the block
        resp.set_data(buf.to_vec());
        let mut response = Response::new();
        response.set_commit(resp);
        let _ = responder.respond(response);
    }
}

fn main() {
    // Run on localhost using default Tendermint port
    env_logger::from_env(Env::default().default_filter_or("info")).init();
    abci::run_local(CounterProxy::new());
}
