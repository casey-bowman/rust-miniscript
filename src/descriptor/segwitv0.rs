// Miniscript
// Written in 2020 by rust-miniscript developers
//
// To the extent possible under law, the author(s) have dedicated all
// copyright and related and neighboring rights to this software to
// the public domain worldwide. This software is distributed without
// any warranty.
//
// You should have received a copy of the CC0 Public Domain Dedication
// along with this software.
// If not, see <http://creativecommons.org/publicdomain/zero/1.0/>.
//

//! # Segwit Output Descriptors
//!
//! Implementation of Segwit Descriptors. Contains the implementation
//! of wsh, wpkh and sortedmulti inside wsh.

use std::fmt;
use std::str::FromStr;

use bitcoin::{self, Address, Network, Script};

use super::checksum::{desc_checksum, verify_checksum};
use super::{DescriptorTrait, SortedMultiVec};
use crate::expression::{self, FromTree};
use crate::miniscript::context::{ScriptContext, ScriptContextError};
use crate::policy::{semantic, Liftable};
use crate::util::varint_len;
use crate::{
    Error, ForEach, ForEachKey, Miniscript, MiniscriptKey, Satisfier, Segwitv0, ToPublicKey,
    TranslatePk,
};
/// A Segwitv0 wsh descriptor
#[derive(Clone, Ord, PartialOrd, Eq, PartialEq, Hash)]
pub struct Wsh<Pk: MiniscriptKey> {
    /// underlying miniscript
    inner: WshInner<Pk>,
}

impl<Pk: MiniscriptKey> Wsh<Pk> {
    /// Get the Inner
    pub fn into_inner(self) -> WshInner<Pk> {
        self.inner
    }

    /// Get a reference to inner
    pub fn as_inner(&self) -> &WshInner<Pk> {
        &self.inner
    }

    /// Create a new wsh descriptor
    pub fn new(ms: Miniscript<Pk, Segwitv0>) -> Result<Self, Error> {
        // do the top-level checks
        Segwitv0::top_level_checks(&ms)?;
        Ok(Self {
            inner: WshInner::Ms(ms),
        })
    }

    /// Create a new sortedmulti wsh descriptor
    pub fn new_sortedmulti(k: usize, pks: Vec<Pk>) -> Result<Self, Error> {
        // The context checks will be carried out inside new function for
        // sortedMultiVec
        Ok(Self {
            inner: WshInner::SortedMulti(SortedMultiVec::new(k, pks)?),
        })
    }

    /// Get the descriptor without the checksum
    pub fn to_string_no_checksum(&self) -> String {
        match self.inner {
            WshInner::SortedMulti(ref smv) => format!("wsh({})", smv),
            WshInner::Ms(ref ms) => format!("wsh({})", ms),
        }
    }

    /// Checks whether the descriptor is safe.
    pub fn sanity_check(&self) -> Result<(), Error> {
        match self.inner {
            WshInner::SortedMulti(ref smv) => smv.sanity_check()?,
            WshInner::Ms(ref ms) => ms.sanity_check()?,
        }
        Ok(())
    }
}

impl<Pk: MiniscriptKey + ToPublicKey> Wsh<Pk> {
    /// Obtain the corresponding script pubkey for this descriptor
    /// Non failing verion of [`DescriptorTrait::script_pubkey`] for this descriptor
    pub fn spk(&self) -> Script {
        self.inner_script().to_v0_p2wsh()
    }

    /// Obtains the corresponding script pubkey for this descriptor.
    pub fn address(&self, network: Network) -> Address {
        match self.inner {
            WshInner::SortedMulti(ref smv) => Address::p2wsh(&smv.encode(), network),
            WshInner::Ms(ref ms) => Address::p2wsh(&ms.encode(), network),
        }
    }

    /// Obtain the underlying miniscript for this descriptor
    /// Non failing verion of [`DescriptorTrait::explicit_script`] for this descriptor
    pub fn inner_script(&self) -> Script {
        match self.inner {
            WshInner::SortedMulti(ref smv) => smv.encode(),
            WshInner::Ms(ref ms) => ms.encode(),
        }
    }

    /// Obtain the pre bip-340 signature script code for this descriptor
    /// Non failing verion of [`DescriptorTrait::script_code`] for this descriptor
    pub fn ecdsa_sighash_script_code(&self) -> Script {
        self.inner_script()
    }
}

/// Wsh Inner
#[derive(Clone, Ord, PartialOrd, Eq, PartialEq, Hash)]
pub enum WshInner<Pk: MiniscriptKey> {
    /// Sorted Multi
    SortedMulti(SortedMultiVec<Pk, Segwitv0>),
    /// Wsh Miniscript
    Ms(Miniscript<Pk, Segwitv0>),
}

impl<Pk: MiniscriptKey> Liftable<Pk> for Wsh<Pk> {
    fn lift(&self) -> Result<semantic::Policy<Pk>, Error> {
        match self.inner {
            WshInner::SortedMulti(ref smv) => smv.lift(),
            WshInner::Ms(ref ms) => ms.lift(),
        }
    }
}

impl<Pk> FromTree for Wsh<Pk>
where
    Pk: MiniscriptKey + FromStr,
    Pk::Hash: FromStr,
    <Pk as FromStr>::Err: ToString,
    <<Pk as MiniscriptKey>::Hash as FromStr>::Err: ToString,
{
    fn from_tree(top: &expression::Tree) -> Result<Self, Error> {
        if top.name == "wsh" && top.args.len() == 1 {
            let top = &top.args[0];
            if top.name == "sortedmulti" {
                return Ok(Wsh {
                    inner: WshInner::SortedMulti(SortedMultiVec::from_tree(top)?),
                });
            }
            let sub = Miniscript::from_tree(top)?;
            Segwitv0::top_level_checks(&sub)?;
            Ok(Wsh {
                inner: WshInner::Ms(sub),
            })
        } else {
            Err(Error::Unexpected(format!(
                "{}({} args) while parsing wsh descriptor",
                top.name,
                top.args.len(),
            )))
        }
    }
}
impl<Pk: MiniscriptKey> fmt::Debug for Wsh<Pk> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self.inner {
            WshInner::SortedMulti(ref smv) => write!(f, "wsh({:?})", smv),
            WshInner::Ms(ref ms) => write!(f, "wsh({:?})", ms),
        }
    }
}

impl<Pk: MiniscriptKey> fmt::Display for Wsh<Pk> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let desc = self.to_string_no_checksum();
        let checksum = desc_checksum(&desc).map_err(|_| fmt::Error)?;
        write!(f, "{}#{}", &desc, &checksum)
    }
}

impl<Pk> FromStr for Wsh<Pk>
where
    Pk: MiniscriptKey + FromStr,
    Pk::Hash: FromStr,
    <Pk as FromStr>::Err: ToString,
    <<Pk as MiniscriptKey>::Hash as FromStr>::Err: ToString,
{
    type Err = Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let desc_str = verify_checksum(s)?;
        let top = expression::Tree::from_str(desc_str)?;
        Wsh::<Pk>::from_tree(&top)
    }
}

impl<Pk: MiniscriptKey> DescriptorTrait<Pk> for Wsh<Pk> {
    fn script_pubkey(&self) -> Script
    where
        Pk: ToPublicKey,
    {
        self.spk()
    }

    fn unsigned_script_sig(&self) -> Script
    where
        Pk: ToPublicKey,
    {
        Script::new()
    }

    fn explicit_script(&self) -> Result<Script, Error>
    where
        Pk: ToPublicKey,
    {
        Ok(self.inner_script())
    }

    fn get_satisfaction<S>(&self, satisfier: S) -> Result<(Vec<Vec<u8>>, Script), Error>
    where
        Pk: ToPublicKey,
        S: Satisfier<Pk>,
    {
        let mut witness = match self.inner {
            WshInner::SortedMulti(ref smv) => smv.satisfy(satisfier)?,
            WshInner::Ms(ref ms) => ms.satisfy(satisfier)?,
        };
        let witness_script = self.inner_script();
        witness.push(witness_script.into_bytes());
        let script_sig = Script::new();
        Ok((witness, script_sig))
    }

    fn get_satisfaction_mall<S>(&self, satisfier: S) -> Result<(Vec<Vec<u8>>, Script), Error>
    where
        Pk: ToPublicKey,
        S: Satisfier<Pk>,
    {
        let mut witness = match self.inner {
            WshInner::SortedMulti(ref smv) => smv.satisfy(satisfier)?,
            WshInner::Ms(ref ms) => ms.satisfy_malleable(satisfier)?,
        };
        witness.push(self.inner_script().into_bytes());
        let script_sig = Script::new();
        Ok((witness, script_sig))
    }

    fn max_satisfaction_weight(&self) -> Result<usize, Error> {
        let (script_size, max_sat_elems, max_sat_size) = match self.inner {
            WshInner::SortedMulti(ref smv) => (
                smv.script_size(),
                smv.max_satisfaction_witness_elements(),
                smv.max_satisfaction_size(),
            ),
            WshInner::Ms(ref ms) => (
                ms.script_size(),
                ms.max_satisfaction_witness_elements()?,
                ms.max_satisfaction_size()?,
            ),
        };
        Ok(4 +  // scriptSig length byte
            varint_len(script_size) +
            script_size +
            varint_len(max_sat_elems) +
            max_sat_size)
    }

    fn script_code(&self) -> Result<Script, Error>
    where
        Pk: ToPublicKey,
    {
        Ok(self.ecdsa_sighash_script_code())
    }
}

impl<Pk: MiniscriptKey> ForEachKey<Pk> for Wsh<Pk> {
    fn for_each_key<'a, F: FnMut(ForEach<'a, Pk>) -> bool>(&'a self, pred: F) -> bool
    where
        Pk: 'a,
        Pk::Hash: 'a,
    {
        match self.inner {
            WshInner::SortedMulti(ref smv) => smv.for_each_key(pred),
            WshInner::Ms(ref ms) => ms.for_each_key(pred),
        }
    }
}

impl<P, Q> TranslatePk<P, Q> for Wsh<P>
where
    P: MiniscriptKey,
    Q: MiniscriptKey,
{
    type Output = Wsh<Q>;

    fn translate_pk<Fpk, Fpkh, E>(&self, mut fpk: Fpk, mut fpkh: Fpkh) -> Result<Self::Output, E>
    where
        Fpk: FnMut(&P) -> Result<Q, E>,
        Fpkh: FnMut(&P::Hash) -> Result<Q::Hash, E>,
    {
        let inner = match self.inner {
            WshInner::SortedMulti(ref smv) => WshInner::SortedMulti(smv.translate_pk(&mut fpk)?),
            WshInner::Ms(ref ms) => WshInner::Ms(ms.translate_pk(&mut fpk, &mut fpkh)?),
        };
        Ok(Wsh { inner })
    }
}

/// A bare Wpkh descriptor at top level
#[derive(Clone, Ord, PartialOrd, Eq, PartialEq, Hash)]
pub struct Wpkh<Pk: MiniscriptKey> {
    /// underlying publickey
    pk: Pk,
}

impl<Pk: MiniscriptKey> Wpkh<Pk> {
    /// Create a new Wpkh descriptor
    pub fn new(pk: Pk) -> Result<Self, Error> {
        // do the top-level checks
        if pk.is_uncompressed() {
            Err(Error::ContextError(ScriptContextError::CompressedOnly(
                pk.to_string(),
            )))
        } else {
            Ok(Self { pk })
        }
    }

    /// Get the inner key
    pub fn into_inner(self) -> Pk {
        self.pk
    }

    /// Get the inner key
    pub fn as_inner(&self) -> &Pk {
        &self.pk
    }

    /// Get the descriptor without the checksum
    pub fn to_string_no_checksum(&self) -> String {
        format!("wpkh({})", self.pk)
    }

    /// Checks whether the descriptor is safe.
    pub fn sanity_check(&self) -> Result<(), Error> {
        if self.pk.is_uncompressed() {
            Err(Error::ContextError(ScriptContextError::CompressedOnly(
                self.pk.to_string(),
            )))
        } else {
            Ok(())
        }
    }
}

impl<Pk: MiniscriptKey + ToPublicKey> Wpkh<Pk> {
    /// Obtain the corresponding script pubkey for this descriptor
    /// Non failing verion of [`DescriptorTrait::script_pubkey`] for this descriptor
    pub fn spk(&self) -> Script {
        let addr = Address::p2wpkh(&self.pk.to_public_key(), Network::Bitcoin)
            .expect("wpkh descriptors have compressed keys");
        addr.script_pubkey()
    }

    /// Obtains the corresponding script pubkey for this descriptor.
    pub fn address(&self, network: Network) -> Address {
        Address::p2wpkh(&self.pk.to_public_key(), network)
            .expect("Rust Miniscript types don't allow uncompressed pks in segwit descriptors")
    }

    /// Obtain the underlying miniscript for this descriptor
    /// Non failing verion of [`DescriptorTrait::explicit_script`] for this descriptor
    pub fn inner_script(&self) -> Script {
        self.spk()
    }

    /// Obtain the pre bip-340 signature script code for this descriptor
    /// Non failing verion of [`DescriptorTrait::script_code`] for this descriptor
    pub fn ecdsa_sighash_script_code(&self) -> Script {
        // For SegWit outputs, it is defined by bip-0143 (quoted below) and is different from
        // the previous txo's scriptPubKey.
        // The item 5:
        //     - For P2WPKH witness program, the scriptCode is `0x1976a914{20-byte-pubkey-hash}88ac`.
        let addr = Address::p2pkh(&self.pk.to_public_key(), Network::Bitcoin);
        addr.script_pubkey()
    }
}

impl<Pk: MiniscriptKey> fmt::Debug for Wpkh<Pk> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "wpkh({:?})", self.pk)
    }
}

impl<Pk: MiniscriptKey> fmt::Display for Wpkh<Pk> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let desc = self.to_string_no_checksum();
        let checksum = desc_checksum(&desc).map_err(|_| fmt::Error)?;
        write!(f, "{}#{}", &desc, &checksum)
    }
}

impl<Pk: MiniscriptKey> Liftable<Pk> for Wpkh<Pk> {
    fn lift(&self) -> Result<semantic::Policy<Pk>, Error> {
        Ok(semantic::Policy::KeyHash(self.pk.to_pubkeyhash()))
    }
}

impl<Pk> FromTree for Wpkh<Pk>
where
    Pk: MiniscriptKey + FromStr,
    Pk::Hash: FromStr,
    <Pk as FromStr>::Err: ToString,
    <<Pk as MiniscriptKey>::Hash as FromStr>::Err: ToString,
{
    fn from_tree(top: &expression::Tree) -> Result<Self, Error> {
        if top.name == "wpkh" && top.args.len() == 1 {
            Ok(Wpkh::new(expression::terminal(&top.args[0], |pk| {
                Pk::from_str(pk)
            })?)?)
        } else {
            Err(Error::Unexpected(format!(
                "{}({} args) while parsing wpkh descriptor",
                top.name,
                top.args.len(),
            )))
        }
    }
}

impl<Pk> FromStr for Wpkh<Pk>
where
    Pk: MiniscriptKey + FromStr,
    Pk::Hash: FromStr,
    <Pk as FromStr>::Err: ToString,
    <<Pk as MiniscriptKey>::Hash as FromStr>::Err: ToString,
{
    type Err = Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let desc_str = verify_checksum(s)?;
        let top = expression::Tree::from_str(desc_str)?;
        Self::from_tree(&top)
    }
}

impl<Pk: MiniscriptKey> DescriptorTrait<Pk> for Wpkh<Pk> {
    fn script_pubkey(&self) -> Script
    where
        Pk: ToPublicKey,
    {
        self.spk()
    }

    fn unsigned_script_sig(&self) -> Script
    where
        Pk: ToPublicKey,
    {
        Script::new()
    }

    fn explicit_script(&self) -> Result<Script, Error>
    where
        Pk: ToPublicKey,
    {
        Ok(self.inner_script())
    }

    fn get_satisfaction<S>(&self, satisfier: S) -> Result<(Vec<Vec<u8>>, Script), Error>
    where
        Pk: ToPublicKey,
        S: Satisfier<Pk>,
    {
        if let Some(sig) = satisfier.lookup_ecdsa_sig(&self.pk) {
            let sig_vec = sig.to_vec();
            let script_sig = Script::new();
            let witness = vec![sig_vec, self.pk.to_public_key().to_bytes()];
            Ok((witness, script_sig))
        } else {
            Err(Error::MissingSig(self.pk.to_public_key()))
        }
    }

    fn get_satisfaction_mall<S>(&self, satisfier: S) -> Result<(Vec<Vec<u8>>, Script), Error>
    where
        Pk: ToPublicKey,
        S: Satisfier<Pk>,
    {
        self.get_satisfaction(satisfier)
    }

    fn max_satisfaction_weight(&self) -> Result<usize, Error> {
        Ok(4 + 1 + 73 + Segwitv0::pk_len(&self.pk))
    }

    fn script_code(&self) -> Result<Script, Error>
    where
        Pk: ToPublicKey,
    {
        Ok(self.ecdsa_sighash_script_code())
    }
}

impl<Pk: MiniscriptKey> ForEachKey<Pk> for Wpkh<Pk> {
    fn for_each_key<'a, F: FnMut(ForEach<'a, Pk>) -> bool>(&'a self, mut pred: F) -> bool
    where
        Pk: 'a,
        Pk::Hash: 'a,
    {
        pred(ForEach::Key(&self.pk))
    }
}

impl<P, Q> TranslatePk<P, Q> for Wpkh<P>
where
    P: MiniscriptKey,
    Q: MiniscriptKey,
{
    type Output = Wpkh<Q>;

    fn translate_pk<Fpk, Fpkh, E>(&self, mut fpk: Fpk, _fpkh: Fpkh) -> Result<Self::Output, E>
    where
        Fpk: FnMut(&P) -> Result<Q, E>,
        Fpkh: FnMut(&P::Hash) -> Result<Q::Hash, E>,
    {
        Ok(Wpkh::new(fpk(&self.pk)?).expect("Uncompressed keys in Wpkh"))
    }
}
