//
/*

*/ 
#![feature(inclusive_range_syntax)] 
#![feature(type_ascription)]
#![feature(more_struct_aliases)]

extern crate modbus_server;
extern crate futures;
extern crate tokio_proto;
extern crate tokio_service;

extern crate docopt;
extern crate rustc_serialize;

use std::sync::{Arc,Mutex};
use std::str;
use futures::{future, Future, BoxFuture,Stream,Sink};
use std::thread;
use std::sync::mpsc::channel;
use docopt::Docopt;
use std::io::{self, ErrorKind, Write,Read};
use tokio_proto::TcpServer;
use tokio_service::Service;
    
use modbus_server::{ModbusTCPProto,ModbusTCPResponse,ModbusTCPRequest};

const USAGE: &'static str = "
Usage: slave [options] <resources> ...

Options:
    --addr=<addr>  # Base URL  [default: 127.0.0.1:502].
";

#[derive(Debug, RustcDecodable)]
struct Args {
    arg_resource: Vec<String>,
    flag_addr: String
}

// TODO: add ModbusRTUCodec

use modbus_server::BlankRegisters;

pub struct ModbusService {
    block:Arc<Mutex<BlankRegisters>>
}

impl ModbusService {
    fn new (
        block:Arc<Mutex<BlankRegisters>>)->ModbusService {
        ModbusService{ block:block}
    }
    
}

impl Service for ModbusService {
    
    type Request = ModbusTCPRequest;
    type Response = ModbusTCPResponse;    
    type Error = io::Error;
    type Future = future::FutureResult<Self::Response, Self::Error>;

    fn call(&self, req: Self::Request) -> Self::Future {
        let mut a = self.block.lock().unwrap();
        future::finished(Self::Response {
            header:req.header,
            pdu:
            a.call(req.pdu)
        })
    }
}

fn main() {
    let mut block = Arc::new(Mutex::new(BlankRegisters::new()));

    let args: Args = Docopt::new(USAGE)
        .and_then(|d| d.decode())
        .unwrap_or_else(|e| {println!("DAMN {:?}",e); e.exit()});
    println!("{:?}", args);
    
    TcpServer::new(ModbusTCPProto, args.flag_addr.parse().unwrap())
        .serve(move || Ok(ModbusService::new(block.clone())));
}

