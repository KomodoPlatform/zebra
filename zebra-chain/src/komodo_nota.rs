//! komodo back sation

use std::ops::Shl;
use std::io::Write;
use byteorder::{LittleEndian, WriteBytesExt};
use zcash_primitives::legacy;

use crate::{block::{Height, self}, transaction, serialization::{ZcashSerialize, SerializationError, ZcashDeserialize}, komodo_hardfork::KOMODO_MAINNET_COIN};

/// temp minimal opcodes list for parsing nota in opreturn
/// hope we will have the complete opcode list in dedicated script source
enum OpCode {
    // push value
    PushData1 = 0x4c,
    // PushData2 = 0x4d,
    // PushData4 = 0x4e,

    // stack ops
    //Dup = 0x76,

    // bit logic
    // Equal = 0x87,
    // EqualVerify = 0x88,

    // crypto
    // Hash160 = 0xa9,
    // CheckSig = 0xac,

    // exit
    OpReturn = 0x6a,
}

/// kmd back notarisation data 
/// Note: no support for asset chains yet
#[derive(Debug, Clone)]
pub struct BackNotarisationData {
    /// last notarised block
    pub notarised_block_hash: block::Hash,

    /// last notarised height
    pub notarised_height: Height,

    /// LTC transaction hash with MoM
    pub tx_hash: transaction::Hash,

    /// notarised chain name (KMD or asset chain)
    pub symbol: String,

    // assets chains data:
    // mom: block::Root,
    // momom: block::Root,
    // ccid: u16,
    // mom_depth: u32,
}

impl BackNotarisationData {
    /// Create new notarisation
    pub fn new() -> Self {
        Self {     
            notarised_block_hash: block::Hash([0; 32]),
            notarised_height: Height(0),
            tx_hash: transaction::Hash([0; 32]),
            symbol: String::default(),
        }
    }
}

impl ZcashSerialize for BackNotarisationData {
    fn zcash_serialize<W: std::io::Write>(&self, mut writer: W) -> Result<(), std::io::Error> {
        writer.write_u8(OpCode::OpReturn as u8)?;

        // serialize nota to bytes
        let mut nota_data = Vec::<u8>::new();
        nota_data.write_all(&self.notarised_block_hash.0)?;
        nota_data.write_u32::<LittleEndian>(self.notarised_height.0)?;
        nota_data.write_all(&self.tx_hash.0)?;
        nota_data.write_all(&[self.symbol.as_bytes(), &[0u8]].concat()[..])?; // add trailing 0 as it is required in komodo

        // serialize as script push_data
        let opreturn_data = legacy::Script(vec![]);
        let opreturn_data = opreturn_data.shl(&nota_data[..]);

        writer.write_all(&opreturn_data.0[..])?;

        Ok(())
    }
}

impl ZcashDeserialize for BackNotarisationData {
    fn zcash_deserialize<R: std::io::Read>(
        mut reader: R,
    ) -> Result<Self, SerializationError> {

        let mut bytes = Vec::new();
        reader.read_to_end(&mut bytes)?;

        if bytes.len() < 1 { return Err(SerializationError::Parse("script empty")); }

        if bytes[0] != OpCode::OpReturn as u8 { return Err(SerializationError::Parse("not opreturn data")); }

        let mut off: usize;
        if bytes.len() > 3 && bytes[1] < OpCode::PushData1 as u8 { off = 2; }
        else if bytes.len() > 5 && bytes[1] == OpCode::PushData1 as u8 { off = 4; }
        else { return Err(SerializationError::Parse("opreturn data empty")); }
    
        // check if this is kmd back nota:
        let mut is_kmd_back = false;
        if off + 72 <= bytes.len() {
            const KMD_NAME: [u8;4] = [0x4b, 0x4d, 0x44, 0x00];
            if bytes[off+68..off+72] == KMD_NAME {  // exact comparison including trailing 0
                is_kmd_back = true;
            }
        } 
    
        if !is_kmd_back { return Err(SerializationError::Parse("not KMD network not supported")); } // TODO: parse notas for other chains
    
        let mut nota = BackNotarisationData::new();
    
        if off + 32 >= bytes.len() { return Err(SerializationError::Parse("premature eof")); }
        let hash_bytes: [u8;32] = bytes[off..off+32].try_into().unwrap();
        nota.notarised_block_hash = block::Hash::from(hash_bytes);
        off += 32;
    
        if off + 4 >= bytes.len() { return Err(SerializationError::Parse("premature eof")); }
        let u32_bytes: [u8;4] = bytes[off..off+4].try_into().unwrap();
        let ht = u32::from_le_bytes(u32_bytes);
        nota.notarised_height = Height(ht);
        off += 4;
    
        if off + 32 >= bytes.len() { return Err(SerializationError::Parse("premature eof")); }
        let hash_bytes: [u8;32] = bytes[off..off+32].try_into().unwrap();
        nota.tx_hash = transaction::Hash::from(hash_bytes);
        off += 32;
    
        if off >= bytes.len() { return Err(SerializationError::Parse("premature eof")); }
        // find network name end pos: it is either 0x0's pos or the next pos after the last sym 
        // (we need to exclude trailing 0 bcz String::from_utf8 does not recognize it as a stop sym)
        let end = if let Some(pos0) = bytes.iter().skip(off).position(|&b| b == 0 as u8) { off+pos0 } else { bytes.len() };
        if let Ok(symbol) = String::from_utf8(bytes[off..end].to_vec()) { 
            nota.symbol = symbol;
        }
        else {
            return Err(SerializationError::Parse("could not nota symbol"));
        }
        if nota.symbol != String::from(KOMODO_MAINNET_COIN) { return Err(SerializationError::Parse("nota symbol not KMD")); }
    
        trace!("komodo found nota {:?}", nota);
        Ok(nota)
    }
}

/// komodo read write KMD back notarisation
#[test]
fn komodo_parse_nota_test() {
    // tx aea56dbf923fee5ebd4193156cfe1ab0f8a770e513b86a6b4da8702b815883a2
    let opreturn_bytes = hex::decode("6a48aac9f7d4f0520acba35ad8a9e6d11b21b309aadafcbed73646cfed2329cb0503b6c62d007ee5cbcdd4519225924dbf19356b7ab945f5e6f769b2915b212a3843182a54164b4d4400").expect("valid nota in hex");

    let nota = BackNotarisationData::zcash_deserialize(&opreturn_bytes[..]).expect("nota parsed okay");

    // println!("{:?} {:?} {:?} {:?}", nota.notarised_height, nota.notarised_block_hash, nota.tx_hash, nota.symbol);

    assert_eq!(nota.notarised_height, Height(2999990));
    assert_eq!(
        format!("{}", nota.notarised_block_hash),
        "0305cb2923edcf4636d7befcdaaa09b3211bd1e6a9d85aa3cb0a52f0d4f7c9aa"
    );
    assert_eq!(
        format!("{}", nota.tx_hash),
        "16542a1843382a215b91b269f7e6f545b97a6b3519bf4d92259251d4cdcbe57e"  
    );
    assert_eq!(nota.symbol, "KMD");
    
    let mut encoded = Vec::new();
    nota.zcash_serialize(&mut encoded).unwrap();
    assert_eq!(&opreturn_bytes, &encoded[..]);
}