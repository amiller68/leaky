use libipld::Ipld;

pub trait Blockable: Sized {
    type Error;

    fn to_ipld(&self) -> Ipld;

    fn from_ipld(ipld: &Ipld) -> Result<Self, Self::Error>;
}
