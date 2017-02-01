//

use BlankRegisters;
use ModbusRequestPDU;
use ModbusResponsePDU;
use FunctionCode;

#[cfg(test)]
mod tests {
    use super::{BlankRegisters,ModbusRequestPDU,ModbusResponsePDU,FunctionCode};
    #[test]
    fn test_read_coils(){
        let mut br = BlankRegisters::new();
        let req = ModbusRequestPDU {
            code: FunctionCode::ReadCoils as u8,
            address: 1 as u16,
            q_or_v:10 as u16,
            addl:None
        };
        let resp = br.call(req.clone());
        match resp {
            ModbusResponsePDU::ReadCoilsResponse {
                code : code,
                byte_count: byte_count,
                coil_status:coil_status   }=> {
                assert!( code == req.code);
                assert!( coil_status.len() == (req.q_or_v /8 +1)  as usize);
                for i in 0..byte_count {
                    assert!( 0 as u8 == coil_status[i as usize]);
                }
            },
            _  => {
                assert!(false);
            }
        };
        
    }
}
