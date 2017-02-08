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
extern crate tokio_core;

extern crate docopt;
extern crate rustc_serialize;
use futures::Future;
use std::str;
use futures::{future, Stream,Sink};
use std::thread;
use futures::sync::mpsc;
use futures::sync::oneshot;

use docopt::Docopt;
use std::io::{self};
use tokio_proto::TcpServer;
use tokio_service::Service;
use tokio_core::reactor::Core;
    
use modbus_server::{ModbusTCPProto,ModbusTCPResponse,ModbusTCPRequest};
use modbus_server::{ModbusRequestPDU,ModbusResponsePDU};

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
    in_: mpsc::Sender<(ModbusRequestPDU,
                       oneshot::Sender<ModbusResponsePDU>)>
}

impl ModbusService {
    fn new (
        in_:mpsc::Sender<(ModbusRequestPDU,
                          oneshot::Sender<ModbusResponsePDU>)>
        )->ModbusService {
        ModbusService{ in_:in_}
    }
    
}

impl Service for ModbusService {
    
    type Request = ModbusTCPRequest;
    type Response = ModbusTCPResponse;    
    type Error = io::Error;
    type Future = future::FutureResult<Self::Response, Self::Error>;

    fn call(&self, req: Self::Request) -> Self::Future {

        let (resp_in,out)=oneshot ::channel::<ModbusResponsePDU>();
        let tmp = self.in_.clone();
        tmp.send((req.pdu.clone(), resp_in)).poll();

        let rx = out.map(|pdu|{

            ModbusTCPResponse {
                header:req.header.clone(),
                pdu:pdu
            }
        });
        
        let w = rx.wait().unwrap();
        future::finished(w)
    }
}

fn main() {

    let args: Args = Docopt::new(USAGE)
        .and_then(|d| d.decode())
        .unwrap_or_else(|e| {println!("DAMN {:?}",e); e.exit()});
    println!("{:?}", args);
    let (in_,req_out)= mpsc::channel::<(ModbusRequestPDU,oneshot::Sender<ModbusResponsePDU>)>(1);
    thread::spawn(move ||{
        
        let mut block = BlankRegisters::new();
        let mut core = Core::new().unwrap();
        core.run(req_out.map(
            |(req, tx)|{
                println!("Sending {:?}",req);
                tx.complete(block.call(req))
            }).for_each(|e| Ok(()))).unwrap();
    });
    

    TcpServer::new(ModbusTCPProto, args.flag_addr.parse().unwrap())
        .serve(move || Ok(ModbusService::new(in_.clone())));
}

