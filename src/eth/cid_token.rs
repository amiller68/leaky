use cid::Cid;
use ethers::abi::{InvalidOutputType, Tokenizable};

// TODO: I should just write a boilerplate wrapper class for Cids for doing just this
// i have to do stuff like this often enough

pub struct CidToken(Cid);

impl From<Cid> for CidToken {
    fn from(cid: Cid) -> Self {
        Self(cid)
    }
}

impl From<CidToken> for Cid {
    fn from(val: CidToken) -> Self {
        val.0
    }
}

impl Tokenizable for CidToken {
    fn from_token(token: ethers::abi::Token) -> Result<Self, InvalidOutputType> {
        let array = match token {
            ethers::abi::Token::FixedArray(array) => array,
            _ => return Err(InvalidOutputType("Invalid Array -- no parse".to_string())),
        };

        // Assert that the array has two FixedBytes tokens
        if array.len() != 2 {
            return Err(InvalidOutputType("Invalid Array -- wrong len".to_string()));
        }

        let bytes_1 = match array.first() {
            Some(ethers::abi::Token::FixedBytes(bytes)) => bytes,
            _ => return Err(InvalidOutputType("Invalid Bytes -- ind 0".to_string())),
        };
        let bytes_2 = match array.get(1) {
            Some(ethers::abi::Token::FixedBytes(bytes)) => bytes,
            _ => return Err(InvalidOutputType("Invalid Bytes -- ind 1".to_string())),
        };

        let mut all_bytes = bytes_1.clone();
        all_bytes.extend(bytes_2);

        let cid = Cid::try_from(all_bytes.as_slice())
            .map_err(|_| InvalidOutputType("Invalid CID -- no parse".to_string()))?;
        Ok(Self(cid))
    }

    fn into_token(self) -> ethers::abi::Token {
        // Split the cid into two FixedBytes tokens of 32 bytes each
        let buff_1 = [0u8; 32];
        let buff_2 = [0u8; 32];
        let bytes = self.0.to_bytes();
        let all_bytes = bytes
            .iter()
            .chain(buff_1.iter())
            .chain(buff_2.iter())
            .take(64)
            .copied()
            .collect::<Vec<u8>>();
        let (bytes_1, bytes_2) = all_bytes.split_at(32);
        let token_1 = ethers::abi::Token::FixedBytes(bytes_1.to_vec());
        let token_2 = ethers::abi::Token::FixedBytes(bytes_2.to_vec());
        // Return a FixedArray token of the two FixedBytes tokens
        ethers::abi::Token::FixedArray(vec![token_1, token_2])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cid_wrapper_rt() {
        let cid = Cid::default();
        let cid_token = CidToken(cid);
        let token = cid_token.into_token();
        let from_cid = CidToken::from_token(token).unwrap();
        assert_eq!(cid, from_cid.into());
    }
}
