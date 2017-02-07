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
use futures::{future};
use std::collections::HashMap;
use docopt::Docopt;
use std::io::{self};
use tokio_proto::TcpServer;
use tokio_service::Service;
    
use modbus_server::{ModbusTCPProto,ModbusTCPResponse,ModbusTCPRequest};

const USAGE: &'static str = "
Usage: multiblock [options] <resource>...

Options:
    --addr=<addr>  # Base URL  [default: 127.0.0.1:502].
";

#[derive(Debug, RustcDecodable)]
struct Args {
    arg_resource: Vec<u8>,
    flag_addr: String
}

// TODO: add ModbusRTUCodec

use modbus_server::BlankRegisters;

pub struct ModbusService {
    blocks:HashMap<u8,Arc<Mutex<BlankRegisters>>>
}

impl ModbusService {
    fn new (
        blocks:HashMap<u8,Arc<Mutex<BlankRegisters>>>)->ModbusService {
        ModbusService{ blocks:blocks}
    }
    
}

impl Service for ModbusService {
    
    type Request = ModbusTCPRequest;
    type Response = ModbusTCPResponse;    
    type Error = io::Error;
    type Future = future::FutureResult<Self::Response, Self::Error>;

    fn call(&self, req: Self::Request) -> Self::Future {
        let mut a = self.blocks[&req.header.uid].lock().unwrap();
        future::finished(Self::Response {
            header:req.header,
            pdu:
            a.call(req.pdu)
        })
    }
}


fn main() {

    let args: Args = Docopt::new(USAGE)
        .and_then(|d| d.decode())
        .unwrap_or_else(|e| {println!("DAMN {:?}",e); e.exit()});
    println!("{:?}", args);
    
    let mut blocks : HashMap<u8,Arc<Mutex<BlankRegisters>>> = HashMap::new();
    for r in args.arg_resource {
        let block = Arc::new(Mutex::new(BlankRegisters::new()));
        blocks.insert(r,block);
    }


    
    TcpServer::new(ModbusTCPProto, args.flag_addr.parse().unwrap())
        .serve(move || Ok(ModbusService::new(blocks.clone())));
}

