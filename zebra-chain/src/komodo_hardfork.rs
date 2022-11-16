//! Notary season pubkeys and functions to access them 

use crate::block::{Height};
use crate::transparent::Output;
use crate::{serialization::DateTime32};
//use chrono::{DateTime, Utc};
use secp256k1::PublicKey;
use thiserror::Error;

use lazy_static::lazy_static;
use std::sync::{Arc, Mutex};
use crate::komodo_utils::{parse_p2pk};

// load NNData
lazy_static! {
    /// static ref to parsed NN pubkeys
    pub static ref NNDATA: Arc<Mutex<NNData<'static>>> = Arc::new(Mutex::new(NNData::new()));   // TODO: do we need a mutex here for the readonly object?
}

const NUM_KMD_NOTARIES: usize = 64;
const NUM_KMD_SEASONS: usize = 7;

/// array of notary pubkeys for a season
type NotarySeasonPubkeys<'a> = Vec<(&'a str, PublicKey)>;

/// notary pubkey id in the season pubkey array
pub type NotaryId = u32;

/// Notarisation constants: HF activation timestamps and heights, notary pubkeys 
#[allow(non_snake_case, dead_code)]
pub struct NNData<'a> {

    nStakedDecemberHardforkTimestamp: DateTime32,   /// activation timestamp
    nDecemberHardforkHeight: Height,                /// activation height

    nS4Timestamp: DateTime32,
    nS4HardforkHeight: Height,

    nS5Timestamp: DateTime32,
    nS5HardforkHeight: Height,

    nS6Timestamp: DateTime32,
    nS6HardforkHeight: Height,

    KMD_SEASON_TIMESTAMPS: Vec<DateTime32>,
    KMD_SEASON_HEIGHTS: Vec<Height>,

    notaries_elected: Vec<NotarySeasonPubkeys<'a>>,
}

#[allow(dead_code, missing_docs)]
#[derive(Error, Debug, PartialEq, Eq)]
pub enum NotaryDataError {

    #[error("no notary pubkeys initialised")]
    NoNotaryPubkeysInitialised,

    #[error("no season for height")]
    NoSeasonForHeight,

    #[error("no season for timestamp")]
    NoSeasonForTimestamp,
}

// Era array of pubkeys. Add extra seasons to bottom as requried, after adding appropriate info above. 
const NOTARIES_ELECTED_SOURCE: [[(&'static str, &'static str); NUM_KMD_NOTARIES]; NUM_KMD_SEASONS] =
[
    [
        ( "0_jl777_testA", "03b7621b44118017a16043f19b30cc8a4cfe068ac4e42417bae16ba460c80f3828" ),
         ( "0_jl777_testB", "02ebfc784a4ba768aad88d44d1045d240d47b26e248cafaf1c5169a42d7a61d344" ),
        ( "0_kolo_testA", "0287aa4b73988ba26cf6565d815786caf0d2c4af704d7883d163ee89cd9977edec" ),
        ( "artik_AR", "029acf1dcd9f5ff9c455f8bb717d4ae0c703e089d16cf8424619c491dff5994c90" ),
        ( "artik_EU", "03f54b2c24f82632e3cdebe4568ba0acf487a80f8a89779173cdb78f74514847ce" ),
        ( "artik_NA", "0224e31f93eff0cc30eaf0b2389fbc591085c0e122c4d11862c1729d090106c842" ),
        ( "artik_SH", "02bdd8840a34486f38305f311c0e2ae73e84046f6e9c3dd3571e32e58339d20937" ),
        ( "badass_EU", "0209d48554768dd8dada988b98aca23405057ac4b5b46838a9378b95c3e79b9b9e" ),
        ( "badass_NA", "02afa1a9f948e1634a29dc718d218e9d150c531cfa852843a1643a02184a63c1a7" ),
        ( "badass_SH", "026b49dd3923b78a592c1b475f208e23698d3f085c4c3b4906a59faf659fd9530b" ),
        ( "crackers_EU", "03bc819982d3c6feb801ec3b720425b017d9b6ee9a40746b84422cbbf929dc73c3" ), // 10
        ( "crackers_NA", "03205049103113d48c7c7af811b4c8f194dafc43a50d5313e61a22900fc1805b45" ),
        ( "crackers_SH", "02be28310e6312d1dd44651fd96f6a44ccc269a321f907502aae81d246fabdb03e" ),
        ( "durerus_EU", "02bcbd287670bdca2c31e5d50130adb5dea1b53198f18abeec7211825f47485d57" ),
        ( "etszombi_AR", "031c79168d15edabf17d9ec99531ea9baa20039d0cdc14d9525863b83341b210e9" ),
        ( "etszombi_EU", "0281b1ad28d238a2b217e0af123ce020b79e91b9b10ad65a7917216eda6fe64bf7" ), // 15
        ( "etszombi_SH", "025d7a193c0757f7437fad3431f027e7b5ed6c925b77daba52a8755d24bf682dde" ),
        ( "farl4web_EU", "0300ecf9121cccf14cf9423e2adb5d98ce0c4e251721fa345dec2e03abeffbab3f" ),
        ( "farl4web_SH", "0396bb5ed3c57aa1221d7775ae0ff751e4c7dc9be220d0917fa8bbdf670586c030" ),
        ( "fullmoon_AR", "0254b1d64840ce9ff6bec9dd10e33beb92af5f7cee628f999cb6bc0fea833347cc" ),
        ( "fullmoon_NA", "031fb362323b06e165231c887836a8faadb96eda88a79ca434e28b3520b47d235b" ), // 20
        ( "fullmoon_SH", "030e12b42ec33a80e12e570b6c8274ce664565b5c3da106859e96a7208b93afd0d" ),
        ( "grewal_NA", "03adc0834c203d172bce814df7c7a5e13dc603105e6b0adabc942d0421aefd2132" ),
        ( "grewal_SH", "03212a73f5d38a675ee3cdc6e82542a96c38c3d1c79d25a1ed2e42fcf6a8be4e68" ),
        ( "indenodes_AR", "02ec0fa5a40f47fd4a38ea5c89e375ad0b6ddf4807c99733c9c3dc15fb978ee147" ),
        ( "indenodes_EU", "0221387ff95c44cb52b86552e3ec118a3c311ca65b75bf807c6c07eaeb1be8303c" ),
        ( "indenodes_NA", "02698c6f1c9e43b66e82dbb163e8df0e5a2f62f3a7a882ca387d82f86e0b3fa988" ),
        ( "indenodes_SH", "0334e6e1ec8285c4b85bd6dae67e17d67d1f20e7328efad17ce6fd24ae97cdd65e" ),
        ( "jeezy_EU", "023cb3e593fb85c5659688528e9a4f1c4c7f19206edc7e517d20f794ba686fd6d6" ),
        ( "jsgalt_NA", "027b3fb6fede798cd17c30dbfb7baf9332b3f8b1c7c513f443070874c410232446" ),
        ( "karasugoi_NA", "02a348b03b9c1a8eac1b56f85c402b041c9bce918833f2ea16d13452309052a982" ), // 30
        ( "kashifali_EU", "033777c52a0190f261c6f66bd0e2bb299d30f012dcb8bfff384103211edb8bb207" ),
        ( "kolo_AR", "03016d19344c45341e023b72f9fb6e6152fdcfe105f3b4f50b82a4790ff54e9dc6" ),
        ( "kolo_SH", "02aa24064500756d9b0959b44d5325f2391d8e95c6127e109184937152c384e185" ),
        ( "metaphilibert_AR", "02adad675fae12b25fdd0f57250b0caf7f795c43f346153a31fe3e72e7db1d6ac6" ),
        ( "movecrypto_AR", "022783d94518e4dc77cbdf1a97915b29f427d7bc15ea867900a76665d3112be6f3" ),
        ( "movecrypto_EU", "021ab53bc6cf2c46b8a5456759f9d608966eff87384c2b52c0ac4cc8dd51e9cc42" ),
        ( "movecrypto_NA", "02efb12f4d78f44b0542d1c60146738e4d5506d27ec98a469142c5c84b29de0a80" ),
        ( "movecrypto_SH", "031f9739a3ebd6037a967ce1582cde66e79ea9a0551c54731c59c6b80f635bc859" ),
        ( "muros_AR", "022d77402fd7179335da39479c829be73428b0ef33fb360a4de6890f37c2aa005e" ),
        ( "noashh_AR", "029d93ef78197dc93892d2a30e5a54865f41e0ca3ab7eb8e3dcbc59c8756b6e355" ), // 40
        ( "noashh_EU", "02061c6278b91fd4ac5cab4401100ffa3b2d5a277e8f71db23401cc071b3665546" ),
        ( "noashh_NA", "033c073366152b6b01535e15dd966a3a8039169584d06e27d92a69889b720d44e1" ),
        ( "nxtswe_EU", "032fb104e5eaa704a38a52c126af8f67e870d70f82977e5b2f093d5c1c21ae5899" ),
        ( "polycryptoblog_NA", "02708dcda7c45fb54b78469673c2587bfdd126e381654819c4c23df0e00b679622" ),
        ( "pondsea_AR", "032e1c213787312099158f2d74a89e8240a991d162d4ce8017d8504d1d7004f735" ),
        ( "pondsea_EU", "0225aa6f6f19e543180b31153d9e6d55d41bc7ec2ba191fd29f19a2f973544e29d" ),
        ( "pondsea_NA", "031bcfdbb62268e2ff8dfffeb9ddff7fe95fca46778c77eebff9c3829dfa1bb411" ),
        ( "pondsea_SH", "02209073bc0943451498de57f802650311b1f12aa6deffcd893da198a544c04f36" ),
        ( "popcornbag_AR", "02761f106fb34fbfc5ddcc0c0aa831ed98e462a908550b280a1f7bd32c060c6fa3" ),
        ( "popcornbag_NA", "03c6085c7fdfff70988fda9b197371f1caf8397f1729a844790e421ee07b3a93e8" ), // 50
        ( "ptytrader_NA", "0328c61467148b207400b23875234f8a825cce65b9c4c9b664f47410b8b8e3c222" ),
        ( "ptytrader_SH", "0250c93c492d8d5a6b565b90c22bee07c2d8701d6118c6267e99a4efd3c7748fa4" ),
        ( "rnr_AR", "029bdb08f931c0e98c2c4ba4ef45c8e33a34168cb2e6bf953cef335c359d77bfcd" ),
        ( "rnr_EU", "03f5c08dadffa0ffcafb8dd7ffc38c22887bd02702a6c9ac3440deddcf2837692b" ),
        ( "rnr_NA", "02e17c5f8c3c80f584ed343b8dcfa6d710dfef0889ec1e7728ce45ce559347c58c" ),
        ( "rnr_SH", "037536fb9bdfed10251f71543fb42679e7c52308bcd12146b2568b9a818d8b8377" ),
        ( "titomane_AR", "03cda6ca5c2d02db201488a54a548dbfc10533bdc275d5ea11928e8d6ab33c2185" ),
        ( "titomane_EU", "02e41feded94f0cc59f55f82f3c2c005d41da024e9a805b41105207ef89aa4bfbd" ),
        ( "titomane_SH", "035f49d7a308dd9a209e894321f010d21b7793461b0c89d6d9231a3fe5f68d9960" ),
        ( "vanbreuk_EU", "024f3cad7601d2399c131fd070e797d9cd8533868685ddbe515daa53c2e26004c3" ), // 60
        ( "xrobesx_NA", "03f0cc6d142d14a40937f12dbd99dbd9021328f45759e26f1877f2a838876709e1" ),
        ( "xxspot1_XX", "02ef445a392fcaf3ad4176a5da7f43580e8056594e003eba6559a713711a27f955" ),
        ( "xxspot2_XX", "03d85b221ea72ebcd25373e7961f4983d12add66a92f899deaf07bab1d8b6f5573" )
    ],
    [
        ("0dev1_jl777", "03b7621b44118017a16043f19b30cc8a4cfe068ac4e42417bae16ba460c80f3828" ),
        ("0dev2_kolo", "030f34af4b908fb8eb2099accb56b8d157d49f6cfb691baa80fdd34f385efed961" ),
        ("0dev3_kolo", "025af9d2b2a05338478159e9ac84543968fd18c45fd9307866b56f33898653b014" ),
        ("0dev4_decker", "028eea44a09674dda00d88ffd199a09c9b75ba9782382cc8f1e97c0fd565fe5707" ),
        ("a-team_SH", "03b59ad322b17cb94080dc8e6dc10a0a865de6d47c16fb5b1a0b5f77f9507f3cce" ),
        ("artik_AR", "029acf1dcd9f5ff9c455f8bb717d4ae0c703e089d16cf8424619c491dff5994c90" ),
        ("artik_EU", "03f54b2c24f82632e3cdebe4568ba0acf487a80f8a89779173cdb78f74514847ce" ),
        ("artik_NA", "0224e31f93eff0cc30eaf0b2389fbc591085c0e122c4d11862c1729d090106c842" ),
        ("artik_SH", "02bdd8840a34486f38305f311c0e2ae73e84046f6e9c3dd3571e32e58339d20937" ),
        ("badass_EU", "0209d48554768dd8dada988b98aca23405057ac4b5b46838a9378b95c3e79b9b9e" ),
        ("badass_NA", "02afa1a9f948e1634a29dc718d218e9d150c531cfa852843a1643a02184a63c1a7" ), // 10
        ("batman_AR", "033ecb640ec5852f42be24c3bf33ca123fb32ced134bed6aa2ba249cf31b0f2563" ),
        ("batman_SH", "02ca5898931181d0b8aafc75ef56fce9c43656c0b6c9f64306e7c8542f6207018c" ),
        ("ca333_EU", "03fc87b8c804f12a6bd18efd43b0ba2828e4e38834f6b44c0bfee19f966a12ba99" ),
        ("chainmakers_EU", "02f3b08938a7f8d2609d567aebc4989eeded6e2e880c058fdf092c5da82c3bc5ee" ),
        ("chainmakers_NA", "0276c6d1c65abc64c8559710b8aff4b9e33787072d3dda4ec9a47b30da0725f57a" ),
        ("chainstrike_SH", "0370bcf10575d8fb0291afad7bf3a76929734f888228bc49e35c5c49b336002153" ),
        ("cipi_AR", "02c4f89a5b382750836cb787880d30e23502265054e1c327a5bfce67116d757ce8" ),
        ("cipi_NA", "02858904a2a1a0b44df4c937b65ee1f5b66186ab87a751858cf270dee1d5031f18" ),
        ("crackers_EU", "03bc819982d3c6feb801ec3b720425b017d9b6ee9a40746b84422cbbf929dc73c3" ),
        ("crackers_NA", "03205049103113d48c7c7af811b4c8f194dafc43a50d5313e61a22900fc1805b45" ), // 20
        ("dwy_EU", "0259c646288580221fdf0e92dbeecaee214504fdc8bbdf4a3019d6ec18b7540424" ),
        ("emmanux_SH", "033f316114d950497fc1d9348f03770cd420f14f662ab2db6172df44c389a2667a" ),
        ("etszombi_EU", "0281b1ad28d238a2b217e0af123ce020b79e91b9b10ad65a7917216eda6fe64bf7" ),
        ("fullmoon_AR", "03380314c4f42fa854df8c471618751879f9e8f0ff5dbabda2bd77d0f96cb35676" ),
        ("fullmoon_NA", "030216211d8e2a48bae9e5d7eb3a42ca2b7aae8770979a791f883869aea2fa6eef" ),
        ("fullmoon_SH", "03f34282fa57ecc7aba8afaf66c30099b5601e98dcbfd0d8a58c86c20d8b692c64" ),
        ("goldenman_EU", "02d6f13a8f745921cdb811e32237bb98950af1a5952be7b3d429abd9152f8e388d" ),
        ("indenodes_AR", "02ec0fa5a40f47fd4a38ea5c89e375ad0b6ddf4807c99733c9c3dc15fb978ee147" ),
        ("indenodes_EU", "0221387ff95c44cb52b86552e3ec118a3c311ca65b75bf807c6c07eaeb1be8303c" ),
        ("indenodes_NA", "02698c6f1c9e43b66e82dbb163e8df0e5a2f62f3a7a882ca387d82f86e0b3fa988" ), // 30
        ("indenodes_SH", "0334e6e1ec8285c4b85bd6dae67e17d67d1f20e7328efad17ce6fd24ae97cdd65e" ),
        ("jackson_AR", "038ff7cfe34cb13b524e0941d5cf710beca2ffb7e05ddf15ced7d4f14fbb0a6f69" ),
        ("jeezy_EU", "023cb3e593fb85c5659688528e9a4f1c4c7f19206edc7e517d20f794ba686fd6d6" ),
        ("karasugoi_NA", "02a348b03b9c1a8eac1b56f85c402b041c9bce918833f2ea16d13452309052a982" ),
        ("komodoninja_EU", "038e567b99806b200b267b27bbca2abf6a3e8576406df5f872e3b38d30843cd5ba" ),
        ("komodoninja_SH", "033178586896915e8456ebf407b1915351a617f46984001790f0cce3d6f3ada5c2" ),
        ("komodopioneers_SH", "033ace50aedf8df70035b962a805431363a61cc4e69d99d90726a2d48fb195f68c" ),
        ("libscott_SH", "03301a8248d41bc5dc926088a8cf31b65e2daf49eed7eb26af4fb03aae19682b95" ),
        ("lukechilds_AR", "031aa66313ee024bbee8c17915cf7d105656d0ace5b4a43a3ab5eae1e14ec02696" ),
        ("madmax_AR", "03891555b4a4393d655bf76f0ad0fb74e5159a615b6925907678edc2aac5e06a75" ), // 40
        ("meshbits_AR", "02957fd48ae6cb361b8a28cdb1b8ccf5067ff68eb1f90cba7df5f7934ed8eb4b2c" ),
        ("meshbits_SH", "025c6e94877515dfd7b05682b9cc2fe4a49e076efe291e54fcec3add78183c1edb" ),
        ("metaphilibert_AR", "02adad675fae12b25fdd0f57250b0caf7f795c43f346153a31fe3e72e7db1d6ac6" ),
        ("metaphilibert_SH", "0284af1a5ef01503e6316a2ca4abf8423a794e9fc17ac6846f042b6f4adedc3309" ),
        ("patchkez_SH", "0296270f394140640f8fa15684fc11255371abb6b9f253416ea2734e34607799c4" ),
        ("pbca26_NA", "0276aca53a058556c485bbb60bdc54b600efe402a8b97f0341a7c04803ce204cb5" ),
        ("peer2cloud_AR", "034e5563cb885999ae1530bd66fab728e580016629e8377579493b386bf6cebb15" ),
        ("peer2cloud_SH", "03396ac453b3f23e20f30d4793c5b8ab6ded6993242df4f09fd91eb9a4f8aede84" ),
        ("polycryptoblog_NA", "02708dcda7c45fb54b78469673c2587bfdd126e381654819c4c23df0e00b679622" ),
        ("hyper_AR", "020f2f984d522051bd5247b61b080b4374a7ab389d959408313e8062acad3266b4" ), // 50
        ("hyper_EU", "03d00cf9ceace209c59fb013e112a786ad583d7de5ca45b1e0df3b4023bb14bf51" ),
        ("hyper_SH", "0383d0b37f59f4ee5e3e98a47e461c861d49d0d90c80e9e16f7e63686a2dc071f3" ),
        ("hyper_NA", "03d91c43230336c0d4b769c9c940145a8c53168bf62e34d1bccd7f6cfc7e5592de" ),
        ("popcornbag_AR", "02761f106fb34fbfc5ddcc0c0aa831ed98e462a908550b280a1f7bd32c060c6fa3" ),
        ("popcornbag_NA", "03c6085c7fdfff70988fda9b197371f1caf8397f1729a844790e421ee07b3a93e8" ),
        ("alien_AR", "0348d9b1fc6acf81290405580f525ee49b4749ed4637b51a28b18caa26543b20f0" ),
        ("alien_EU", "020aab8308d4df375a846a9e3b1c7e99597b90497efa021d50bcf1bbba23246527" ),
        ("thegaltmines_NA", "031bea28bec98b6380958a493a703ddc3353d7b05eb452109a773eefd15a32e421" ),
        ("titomane_AR", "029d19215440d8cb9cc6c6b7a4744ae7fb9fb18d986e371b06aeb34b64845f9325" ),
        ("titomane_EU", "0360b4805d885ff596f94312eed3e4e17cb56aa8077c6dd78d905f8de89da9499f" ), // 60
        ("titomane_SH", "03573713c5b20c1e682a2e8c0f8437625b3530f278e705af9b6614de29277a435b" ),
        ("webworker01_NA", "03bb7d005e052779b1586f071834c5facbb83470094cff5112f0072b64989f97d7" ),
        ("xrobesx_NA", "03f0cc6d142d14a40937f12dbd99dbd9021328f45759e26f1877f2a838876709e1" ),
    ],
    [
        ("madmax_NA", "0237e0d3268cebfa235958808db1efc20cc43b31100813b1f3e15cc5aa647ad2c3" ), // 0
        ("alright_AR", "020566fe2fb3874258b2d3cf1809a5d650e0edc7ba746fa5eec72750c5188c9cc9" ),
        ("strob_NA", "0206f7a2e972d9dfef1c424c731503a0a27de1ba7a15a91a362dc7ec0d0fb47685" ),
        ("dwy_EU", "021c7cf1f10c4dc39d13451123707ab780a741feedab6ac449766affe37515a29e" ),
        ("phm87_SH", "021773a38db1bc3ede7f28142f901a161c7b7737875edbb40082a201c55dcf0add" ),
        ("chainmakers_NA", "02285d813c30c0bf7eefdab1ff0a8ad08a07a0d26d8b95b3943ce814ac8e24d885" ),
        ("indenodes_EU", "0221387ff95c44cb52b86552e3ec118a3c311ca65b75bf807c6c07eaeb1be8303c" ),
        ("blackjok3r_SH", "021eac26dbad256cbb6f74d41b10763183ee07fb609dbd03480dd50634170547cc" ),
        ("chainmakers_EU", "03fdf5a3fce8db7dee89724e706059c32e5aa3f233a6b6cc256fea337f05e3dbf7" ),
        ("titomane_AR", "023e3aa9834c46971ff3e7cb86a200ec9c8074a9566a3ea85d400d5739662ee989" ),
        ("fullmoon_SH", "023b7252968ea8a955cd63b9e57dee45a74f2d7ba23b4e0595572138ad1fb42d21" ), // 10
        ("indenodes_NA", "02698c6f1c9e43b66e82dbb163e8df0e5a2f62f3a7a882ca387d82f86e0b3fa988" ),
        ("chmex_EU", "0281304ebbcc39e4f09fda85f4232dd8dacd668e20e5fc11fba6b985186c90086e" ),
        ("metaphilibert_SH", "0284af1a5ef01503e6316a2ca4abf8423a794e9fc17ac6846f042b6f4adedc3309" ),
        ("ca333_DEV", "02856843af2d9457b5b1c907068bef6077ea0904cc8bd4df1ced013f64bf267958" ),
        ("cipi_NA", "02858904a2a1a0b44df4c937b65ee1f5b66186ab87a751858cf270dee1d5031f18" ),
        ("pungocloud_SH", "024dfc76fa1f19b892be9d06e985d0c411e60dbbeb36bd100af9892a39555018f6" ),
        ("voskcoin_EU", "034190b1c062a04124ad15b0fa56dfdf34aa06c164c7163b6aec0d654e5f118afb" ),
        ("decker_DEV", "028eea44a09674dda00d88ffd199a09c9b75ba9782382cc8f1e97c0fd565fe5707" ),
        ("cryptoeconomy_EU", "0290ab4937e85246e048552df3e9a66cba2c1602db76e03763e16c671e750145d1" ),
        ("etszombi_EU", "0293ea48d8841af7a419a24d9da11c34b39127ef041f847651bae6ab14dcd1f6b4" ),  // 20
        ("karasugoi_NA", "02a348b03b9c1a8eac1b56f85c402b041c9bce918833f2ea16d13452309052a982" ),
        ("pirate_AR", "03e29c90354815a750db8ea9cb3c1b9550911bb205f83d0355a061ac47c4cf2fde" ),
        ("metaphilibert_AR", "02adad675fae12b25fdd0f57250b0caf7f795c43f346153a31fe3e72e7db1d6ac6" ),
        ("zatjum_SH", "02d6b0c89cacd58a0af038139a9a90c9e02cd1e33803a1f15fceabea1f7e9c263a" ),
        ("madmax_AR", "03c5941fe49d673c094bc8e9bb1a95766b4670c88be76d576e915daf2c30a454d3" ),
        ("lukechilds_NA", "03f1051e62c2d280212481c62fe52aab0a5b23c95de5b8e9ad5f80d8af4277a64b" ),
        ("cipi_AR", "02c4f89a5b382750836cb787880d30e23502265054e1c327a5bfce67116d757ce8" ),
        ("tonyl_AR", "02cc8bc862f2b65ad4f99d5f68d3011c138bf517acdc8d4261166b0be8f64189e1" ),
        ("infotech_DEV", "0345ad4ab5254782479f6322c369cec77a7535d2f2162d103d666917d5e4f30c4c" ),
        ("fullmoon_NA", "032c716701fe3a6a3f90a97b9d874a9d6eedb066419209eed7060b0cc6b710c60b" ),  // 30
        ("etszombi_AR", "02e55e104aa94f70cde68165d7df3e162d4410c76afd4643b161dea044aa6d06ce" ),
        ("node-9_EU", "0372e5b51e86e2392bb15039bac0c8f975b852b45028a5e43b324c294e9f12e411" ),
        ("phba2061_EU", "03f6bd15dba7e986f0c976ea19d8a9093cb7c989d499f1708a0386c5c5659e6c4e" ),
        ("indenodes_AR", "02ec0fa5a40f47fd4a38ea5c89e375ad0b6ddf4807c99733c9c3dc15fb978ee147" ),
        ("and1-89_EU", "02736cbf8d7b50835afd50a319f162dd4beffe65f2b1dc6b90e64b32c8e7849ddd" ),
        ("komodopioneers_SH", "032a238a5747777da7e819cfa3c859f3677a2daf14e4dce50916fc65d00ad9c52a" ),
        ("komodopioneers_EU", "036d02425916444fff8cc7203fcbfc155c956dda5ceb647505836bef59885b6866" ),
        ("d0ct0r_NA", "0303725d8525b6f969122faf04152653eb4bf34e10de92182263321769c334bf58" ),
        ("kolo_DEV", "02849e12199dcc27ba09c3902686d2ad0adcbfcee9d67520e9abbdda045ba83227" ),
        ("peer2cloud_AR", "02acc001fe1fe8fd68685ba26c0bc245924cb592e10cec71e9917df98b0e9d7c37" ), // 40
        ("webworker01_SH", "031e50ba6de3c16f99d414bb89866e578d963a54bde7916c810608966fb5700776" ),
        ("webworker01_NA", "032735e9cad1bb00eaababfa6d27864fa4c1db0300c85e01e52176be2ca6a243ce" ),
        ("pbca26_NA", "03a97606153d52338bcffd1bf19bb69ef8ce5a7cbdc2dbc3ff4f89d91ea6bbb4dc" ),
        ("indenodes_SH", "0334e6e1ec8285c4b85bd6dae67e17d67d1f20e7328efad17ce6fd24ae97cdd65e" ),
        ("pirate_NA", "0255e32d8a56671dee8aa7f717debb00efa7f0086ee802de0692f2d67ee3ee06ee" ),
        ("lukechilds_AR", "025c6a73ff6d750b9ddf6755b390948cffdd00f344a639472d398dd5c6b4735d23" ),
        ("dragonhound_NA", "0224a9d951d3a06d8e941cc7362b788bb1237bb0d56cc313e797eb027f37c2d375" ),
        ("fullmoon_AR", "03da64dd7cd0db4c123c2f79d548a96095a5a103e5b9d956e9832865818ffa7872" ),
        ("chainzilla_SH", "0360804b8817fd25ded6e9c0b50e3b0782ac666545b5416644198e18bc3903d9f9" ),
        ("titomane_EU", "03772ac0aad6b0e9feec5e591bff5de6775d6132e888633e73d3ba896bdd8e0afb" ), // 50
        ("jeezy_EU", "037f182facbad35684a6e960699f5da4ba89e99f0d0d62a87e8400dd086c8e5dd7" ),
        ("titomane_SH", "03850fdddf2413b51790daf51dd30823addb37313c8854b508ea6228205047ef9b" ),
        ("alien_AR", "03911a60395801082194b6834244fa78a3c30ff3e888667498e157b4aa80b0a65f" ),
        ("pirate_EU", "03fff24efd5648870a23badf46e26510e96d9e79ce281b27cfe963993039dd1351" ),
        ("thegaltmines_NA", "02db1a16c7043f45d6033ccfbd0a51c2d789b32db428902f98b9e155cf0d7910ed" ),
        ("computergenie_NA", "03a78ae070a5e9e935112cf7ea8293f18950f1011694ea0260799e8762c8a6f0a4" ),
        ("nutellalicka_SH", "02f7d90d0510c598ce45915e6372a9cd0ba72664cb65ce231f25d526fc3c5479fc" ),
        ("chainstrike_SH", "03b806be3bf7a1f2f6290ec5c1ea7d3ea57774dcfcf2129a82b2569e585100e1cb" ),
        ("dwy_SH", "036536d2d52d85f630b68b050f29ea1d7f90f3b42c10f8c5cdf3dbe1359af80aff" ),
        ("alien_EU", "03bb749e337b9074465fa28e757b5aa92cb1f0fea1a39589bca91a602834d443cd" ), // 60
        ("gt_AR", "0348430538a4944d3162bb4749d8c5ed51299c2434f3ee69c11a1f7815b3f46135" ),
        ("patchkez_SH", "03f45e9beb5c4cd46525db8195eb05c1db84ae7ef3603566b3d775770eba3b96ee" ),
        ("decker_AR", "03ffdf1a116300a78729608d9930742cd349f11a9d64fcc336b8f18592dd9c91bc" ), // 63
    ],
    [
        // Season 3.5
        ("madmax_NA", "0237e0d3268cebfa235958808db1efc20cc43b31100813b1f3e15cc5aa647ad2c3" ), // 0
        ("alright_AR", "020566fe2fb3874258b2d3cf1809a5d650e0edc7ba746fa5eec72750c5188c9cc9" ),
        ("strob_NA", "0206f7a2e972d9dfef1c424c731503a0a27de1ba7a15a91a362dc7ec0d0fb47685" ),
        ("hunter_EU", "0378224b4e9d8a0083ce36f2963ec0a4e231ec06b0c780de108e37f41181a89f6a" ), // FIXME verify this, kolo. Change name if you want
        ("phm87_SH", "021773a38db1bc3ede7f28142f901a161c7b7737875edbb40082a201c55dcf0add" ),
        ("chainmakers_NA", "02285d813c30c0bf7eefdab1ff0a8ad08a07a0d26d8b95b3943ce814ac8e24d885" ),
        ("indenodes_EU", "0221387ff95c44cb52b86552e3ec118a3c311ca65b75bf807c6c07eaeb1be8303c" ),
        ("blackjok3r_SH", "021eac26dbad256cbb6f74d41b10763183ee07fb609dbd03480dd50634170547cc" ),
        ("chainmakers_EU", "03fdf5a3fce8db7dee89724e706059c32e5aa3f233a6b6cc256fea337f05e3dbf7" ),
        ("titomane_AR", "023e3aa9834c46971ff3e7cb86a200ec9c8074a9566a3ea85d400d5739662ee989" ),
        ("fullmoon_SH", "023b7252968ea8a955cd63b9e57dee45a74f2d7ba23b4e0595572138ad1fb42d21" ), // 10
        ("indenodes_NA", "02698c6f1c9e43b66e82dbb163e8df0e5a2f62f3a7a882ca387d82f86e0b3fa988" ),
        ("chmex_EU", "0281304ebbcc39e4f09fda85f4232dd8dacd668e20e5fc11fba6b985186c90086e" ),
        ("metaphilibert_SH", "0284af1a5ef01503e6316a2ca4abf8423a794e9fc17ac6846f042b6f4adedc3309" ),
        ("ca333_DEV", "02856843af2d9457b5b1c907068bef6077ea0904cc8bd4df1ced013f64bf267958" ),
        ("cipi_NA", "02858904a2a1a0b44df4c937b65ee1f5b66186ab87a751858cf270dee1d5031f18" ),
        ("pungocloud_SH", "024dfc76fa1f19b892be9d06e985d0c411e60dbbeb36bd100af9892a39555018f6" ),
        ("voskcoin_EU", "034190b1c062a04124ad15b0fa56dfdf34aa06c164c7163b6aec0d654e5f118afb" ),
        ("decker_DEV", "028eea44a09674dda00d88ffd199a09c9b75ba9782382cc8f1e97c0fd565fe5707" ),
        ("cryptoeconomy_EU", "0290ab4937e85246e048552df3e9a66cba2c1602db76e03763e16c671e750145d1" ),
        ("etszombi_EU", "0293ea48d8841af7a419a24d9da11c34b39127ef041f847651bae6ab14dcd1f6b4" ),  // 20
        ("karasugoi_NA", "02a348b03b9c1a8eac1b56f85c402b041c9bce918833f2ea16d13452309052a982" ),
        ("pirate_AR", "03e29c90354815a750db8ea9cb3c1b9550911bb205f83d0355a061ac47c4cf2fde" ),
        ("metaphilibert_AR", "02adad675fae12b25fdd0f57250b0caf7f795c43f346153a31fe3e72e7db1d6ac6" ),
        ("zatjum_SH", "02d6b0c89cacd58a0af038139a9a90c9e02cd1e33803a1f15fceabea1f7e9c263a" ),
        ("madmax_AR", "03c5941fe49d673c094bc8e9bb1a95766b4670c88be76d576e915daf2c30a454d3" ),
        ("lukechilds_NA", "03f1051e62c2d280212481c62fe52aab0a5b23c95de5b8e9ad5f80d8af4277a64b" ),
        ("cipi_AR", "02c4f89a5b382750836cb787880d30e23502265054e1c327a5bfce67116d757ce8" ),
        ("tonyl_AR", "02cc8bc862f2b65ad4f99d5f68d3011c138bf517acdc8d4261166b0be8f64189e1" ),
        ("infotech_DEV", "0345ad4ab5254782479f6322c369cec77a7535d2f2162d103d666917d5e4f30c4c" ),
        ("fullmoon_NA", "032c716701fe3a6a3f90a97b9d874a9d6eedb066419209eed7060b0cc6b710c60b" ),  // 30
        ("etszombi_AR", "02e55e104aa94f70cde68165d7df3e162d4410c76afd4643b161dea044aa6d06ce" ),
        ("node-9_EU", "0372e5b51e86e2392bb15039bac0c8f975b852b45028a5e43b324c294e9f12e411" ),
        ("phba2061_EU", "03f6bd15dba7e986f0c976ea19d8a9093cb7c989d499f1708a0386c5c5659e6c4e" ),
        ("indenodes_AR", "02ec0fa5a40f47fd4a38ea5c89e375ad0b6ddf4807c99733c9c3dc15fb978ee147" ),
        ("and1-89_EU", "02736cbf8d7b50835afd50a319f162dd4beffe65f2b1dc6b90e64b32c8e7849ddd" ),
        ("komodopioneers_SH", "032a238a5747777da7e819cfa3c859f3677a2daf14e4dce50916fc65d00ad9c52a" ),
        ("komodopioneers_EU", "036d02425916444fff8cc7203fcbfc155c956dda5ceb647505836bef59885b6866" ),
        ("d0ct0r_NA", "0303725d8525b6f969122faf04152653eb4bf34e10de92182263321769c334bf58" ),
        ("kolo_DEV", "02849e12199dcc27ba09c3902686d2ad0adcbfcee9d67520e9abbdda045ba83227" ),
        ("peer2cloud_AR", "02acc001fe1fe8fd68685ba26c0bc245924cb592e10cec71e9917df98b0e9d7c37" ), // 40
        ("webworker01_SH", "031e50ba6de3c16f99d414bb89866e578d963a54bde7916c810608966fb5700776" ),
        ("webworker01_NA", "032735e9cad1bb00eaababfa6d27864fa4c1db0300c85e01e52176be2ca6a243ce" ),
        ("pbca26_NA", "03a97606153d52338bcffd1bf19bb69ef8ce5a7cbdc2dbc3ff4f89d91ea6bbb4dc" ),
        ("indenodes_SH", "0334e6e1ec8285c4b85bd6dae67e17d67d1f20e7328efad17ce6fd24ae97cdd65e" ),
        ("pirate_NA", "0255e32d8a56671dee8aa7f717debb00efa7f0086ee802de0692f2d67ee3ee06ee" ),
        ("lukechilds_AR", "025c6a73ff6d750b9ddf6755b390948cffdd00f344a639472d398dd5c6b4735d23" ),
        ("dragonhound_NA", "0224a9d951d3a06d8e941cc7362b788bb1237bb0d56cc313e797eb027f37c2d375" ),
        ("fullmoon_AR", "03da64dd7cd0db4c123c2f79d548a96095a5a103e5b9d956e9832865818ffa7872" ),
        ("chainzilla_SH", "0360804b8817fd25ded6e9c0b50e3b0782ac666545b5416644198e18bc3903d9f9" ),
        ("titomane_EU", "03772ac0aad6b0e9feec5e591bff5de6775d6132e888633e73d3ba896bdd8e0afb" ), // 50
        ("jeezy_EU", "037f182facbad35684a6e960699f5da4ba89e99f0d0d62a87e8400dd086c8e5dd7" ),
        ("titomane_SH", "03850fdddf2413b51790daf51dd30823addb37313c8854b508ea6228205047ef9b" ),
        ("alien_AR", "03911a60395801082194b6834244fa78a3c30ff3e888667498e157b4aa80b0a65f" ),
        ("pirate_EU", "03fff24efd5648870a23badf46e26510e96d9e79ce281b27cfe963993039dd1351" ),
        ("thegaltmines_NA", "02db1a16c7043f45d6033ccfbd0a51c2d789b32db428902f98b9e155cf0d7910ed" ),
        ("computergenie_NA", "03a78ae070a5e9e935112cf7ea8293f18950f1011694ea0260799e8762c8a6f0a4" ),
        ("nutellalicka_SH", "02f7d90d0510c598ce45915e6372a9cd0ba72664cb65ce231f25d526fc3c5479fc" ),
        ("chainstrike_SH", "03b806be3bf7a1f2f6290ec5c1ea7d3ea57774dcfcf2129a82b2569e585100e1cb" ),
        ("hunter_SH", "02407db70ad30ce4dfaee8b4ae35fae88390cad2b0ba0373fdd6231967537ccfdf" ), 
        ("alien_EU", "03bb749e337b9074465fa28e757b5aa92cb1f0fea1a39589bca91a602834d443cd" ), // 60
        ("gt_AR", "0348430538a4944d3162bb4749d8c5ed51299c2434f3ee69c11a1f7815b3f46135" ),
        ("patchkez_SH", "03f45e9beb5c4cd46525db8195eb05c1db84ae7ef3603566b3d775770eba3b96ee" ),
        ("decker_AR", "03ffdf1a116300a78729608d9930742cd349f11a9d64fcc336b8f18592dd9c91bc" ), // 63
    ],
    [
        // Season 4
        ( "alien_AR", "03911a60395801082194b6834244fa78a3c30ff3e888667498e157b4aa80b0a65f" ),
        ( "alien_EU", "03bb749e337b9074465fa28e757b5aa92cb1f0fea1a39589bca91a602834d443cd" ),
        ( "strob_NA", "02a1c0bd40b294f06d3e44a52d1b2746c260c475c725e9351f1312e49e01c9a405" ),
        ( "titomane_SH", "020014ad4eedf6b1aeb0ad3b101a58d0a2fc570719e46530fd98d4e585f63eb4ae" ),
        ( "fullmoon_AR", "03b251095e747f759505ec745a4bbff9a768b8dce1f65137300b7c21efec01a07a" ),
        ( "phba2061_EU", "03a9492d2a1601d0d98cfe94d8adf9689d1bb0e600088127a4f6ca937761fb1c66" ),
        ( "fullmoon_NA", "03931c1d654a99658998ce0ddae108d825943a821d1cddd85e948ac1d483f68fb6" ),
        ( "fullmoon_SH", "03c2a1ed9ddb7bb8344328946017b9d8d1357b898957dd6aaa8c190ae26740b9ff" ),
        ( "madmax_AR", "022be5a2829fa0291f9a51ff7aeceef702eef581f2611887c195e29da49092e6de" ),
        ( "titomane_EU", "0285cf1fdba761daf6f1f611c32d319cd58214972ef822793008b69dde239443dd" ),
        ( "cipi_NA", "022c6825a24792cc3b010b1531521eba9b5e2662d640ed700fd96167df37e75239" ),
        ( "indenodes_SH", "0334e6e1ec8285c4b85bd6dae67e17d67d1f20e7328efad17ce6fd24ae97cdd65e" ),
        ( "decker_AR", "03ffdf1a116300a78729608d9930742cd349f11a9d64fcc336b8f18592dd9c91bc" ),
        ( "indenodes_EU", "0221387ff95c44cb52b86552e3ec118a3c311ca65b75bf807c6c07eaeb1be8303c" ),
        ( "madmax_NA", "02997b7ab21b86bbea558ae79acc35d62c9cedf441578f78112f986d72e8eece08" ),
        ( "chainzilla_SH", "02288ba6dc57936b59d60345e397d62f5d7e7d975f34ed5c2f2e23288325661563" ),
        ( "peer2cloud_AR", "0250e7e43a3535731b051d1bcc7dc88fbb5163c3fe41c5dee72bd973bcc4dca9f2" ),
        ( "pirate_EU", "0231c0f50a06655c3d2edf8d7e722d290195d49c78d50de7786b9d196e8820c848" ),
        ( "webworker01_NA", "02dfd5f3cef1142879a7250752feb91ddd722c497fb98c7377c0fcc5ccc201bd55" ),
        ( "zatjum_SH", "036066fd638b10e555597623e97e032b28b4d1fa5a13c2b0c80c420dbddad236c2" ),
        ( "titomane_AR", "0268203a4c80047edcd66385c22e764ea5fb8bc42edae389a438156e7dca9a8251" ),
        ( "chmex_EU", "025b7209ba37df8d9695a23ea706ea2594863ab09055ca6bf485855937f3321d1d" ),
        ( "indenodes_NA", "02698c6f1c9e43b66e82dbb163e8df0e5a2f62f3a7a882ca387d82f86e0b3fa988" ),
        ( "patchkez_SH", "02cabd6c5fc0b5476c7a01e9d7b907e9f0a051d7f4f731959955d3f6b18ee9a242" ),
        ( "metaphilibert_AR", "02adad675fae12b25fdd0f57250b0caf7f795c43f346153a31fe3e72e7db1d6ac6" ),
        ( "etszombi_EU", "0341adbf238f33a33cc895633db996c3ad01275313ac6641e046a3db0b27f1c880" ),
        ( "pirate_NA", "02207f27a13625a0b8caef6a7bb9de613ff16e4a5f232da8d7c235c7c5bad72ffe" ),
        ( "metaphilibert_SH", "0284af1a5ef01503e6316a2ca4abf8423a794e9fc17ac6846f042b6f4adedc3309" ),
        ( "indenodes_AR", "02ec0fa5a40f47fd4a38ea5c89e375ad0b6ddf4807c99733c9c3dc15fb978ee147" ),
        ( "chainmakers_NA", "029415a1609c33dfe4a1016877ba35f9265d25d737649f307048efe96e76512877" ),
        ( "mihailo_EU", "037f9563f30c609b19fd435a19b8bde7d6db703012ba1aba72e9f42a87366d1941" ),
        ( "tonyl_AR", "0299684d7291abf90975fa493bf53212cf1456c374aa36f83cc94daece89350ae9" ),
        ( "alien_NA", "03bea1ac333b95c8669ec091907ea8713cae26f74b9e886e13593400e21c4d30a8" ),
        ( "pungocloud_SH", "025b97d8c23effaca6fa7efacce20bf54df73081b63004a0fe22f3f98fece5669f" ),
        ( "node9_EU", "029ffa793b5c3248f8ea3da47fa3cf1810dada5af032ecd0e37bab5b92dd63b34e" ),
        ( "smdmitry_AR", "022a2a45979a6631a25e4c96469423de720a2f4c849548957c35a35c91041ee7ac" ),
        ( "nodeone_NA", "03f9dd0484e81174fd50775cb9099691c7d140ff00c0f088847e38dc87da67eb9b" ),
        ( "gcharang_SH", "02ec4172eab854a0d8cd32bc691c83e93975a3df5a4a453a866736c56e025dc359" ),
        ( "cipi_EU", "02f2b6defff1c544202f66e47cfd6909c54d67c7c39b9c2a99f137dbaf6d0bd8fa" ),
        ( "etszombi_AR", "0329944b0ac65b6760787ede042a2fde0be9fca1d80dd756bc0ee0b98d389b7682" ),
        ( "pbca26_NA", "0387e0fb6f2ca951154c87e16c6cbf93a69862bb165c1a96bcd8722b3af24fe533" ),
        ( "mylo_SH", "03b58f57822e90fe105e6efb63fd8666033ea503d6cc165b1e479bbd8c2ba033e8" ),
        ( "swisscertifiers_EU", "03ebcc71b42d88994b8b2134bcde6cb269bd7e71a9dd7616371d9294ec1c1902c5" ),
        ( "marmarachain_AR", "035bbd81a098172592fe97f50a0ce13cbbf80e55cc7862eccdbd7310fab8a90c4c" ),
        ( "karasugoi_NA", "0262cf2559703464151153c12e00c4b67a969e39b330301fdcaa6667d7eb02c57d" ),
        ( "phm87_SH", "021773a38db1bc3ede7f28142f901a161c7b7737875edbb40082a201c55dcf0add" ),
        ( "oszy_EU", "03d1ffd680491b98a3ec5541715681d1a45293c8efb1722c32392a1d792622596a" ),
        ( "chmex_AR", "036c856ea778ea105b93c0be187004d4e51161eda32888aa307b8f72d490884005" ),
        ( "dragonhound_NA", "0227e5cad3731e381df157de189527aac8eb50d82a13ce2bd81153984ebc749515" ),
        ( "strob_SH", "025ceac4256cef83ca4b110f837a71d70a5a977ecfdf807335e00bc78b560d451a" ),
        ( "madmax_EU", "02ea0cf4d6d151d0528b07efa79cc7403d77cb9195e2e6c8374f5074b9a787e287" ),
        ( "dudezmobi_AR", "027ecd974ff2a27a37ee69956cd2e6bb31a608116206f3e31ef186823420182450" ),
        ( "daemonfox_NA", "022d6f4885f53cbd668ad7d03d4f8e830c233f74e3a918da1ed247edfc71820b3d" ),
        ( "nutellalicka_SH", "02f4b1e71bc865a79c05fe333952b97cb040d8925d13e83925e170188b3011269b" ),
        ( "starfleet_EU", "025c7275bd750936862b47793f1f0bb3cbed60fb75a48e7da016e557925fe375eb" ),
        ( "mrlynch_AR", "031987dc82b087cd53e23df5480e265a5928e9243e0e11849fa12359739d8b18a4" ),
        ( "greer_NA", "03e0995615d7d3cf1107effa6bdb1133e0876cf1768e923aa533a4e2ee675ec383" ),
        ( "mcrypt_SH", "025faab3cc2e83bf7dad6a9463cbff86c08800e937942126f258cf219bc2320043" ),
        ( "decker_EU", "03777777caebce56e17ca3aae4e16374335b156f1dd62ee3c7f8799c6b885f5560" ),
        ( "dappvader_SH", "02962e2e5af746632016bc7b24d444f7c90141a5f42ce54e361b302cf455d90e6a" ),
        ( "alright_DEV", "02b73a589d61691efa2ada15c006d27bc18493fea867ce6c14db3d3d28751f8ce3" ),
        ( "artemii235_DEV", "03bb616b12430bdd0483653de18733597a4fd416623c7065c0e21fe9d96460add1" ),
        ( "tonyl_DEV", "02d5f7fd6e25d34ab2f3318d60cdb89ff3a812ec5d0212c4c113bb12d12616cfdc" ),
        ( "decker_DEV", "028eea44a09674dda00d88ffd199a09c9b75ba9782382cc8f1e97c0fd565fe5707" )
    ],
    [    
        // Season 5
        ("alrighttt_DEV", "03483166d8663beeb48a493eec161bf506df1906153b6259f7ca617e4cb8110260"), // 0
        ("alien_AR", "03911a60395801082194b6834244fa78a3c30ff3e888667498e157b4aa80b0a65f"),
        ("artempikulin_AR", "026a8ed1e4eeeb023cfb8e003e1c1de6a2b771f37e112745ffb8b6e375a9cbfdec"),
        ("chmex_AR", "036c856ea778ea105b93c0be187004d4e51161eda32888aa307b8f72d490884005"),
        ("cipi_AR", "033ae024cdb748e083406a2e20037017a1292079ad6a8161ae5b43f398724fea74"),
        ("shadowbit_AR", "02909c79a198179c193fb85bbd4ba09b875a5a9bd481fec284658188b96ed43519"),
        ("goldenman_AR", "0345b888e5de9c11871c080212ccaebf8a3d77b05fe3d535336efc5c7df334bbc7"),
        ("kolo_AR", "0281d3c7bf067088b9572b4d906afca2083a71a38b1011878ecd347651d00af433"),
        ("madmax_AR", "02f729b8df4dacdc8d811416eb32e98a5cc37023b42c81b77d1c00881de879a99a"),
        ("mcrypt_AR", "029bdb33b08f96524082490f4373bc6026b92bcaef9bc521a840a799c73b75ed80"),
        ("mrlynch_AR", "031987dc82b087cd53e23df5480e265a5928e9243e0e11849fa12359739d8b18a4"), // 10
        ("ocean_AR", "03c2bc8c57a001a788851fedc33ce72797ee8fe26eaa3abb1b807727e4867a3105"),
        ("smdmitry_AR", "022a2a45979a6631a25e4c96469423de720a2f4c849548957c35a35c91041ee7ac"),
        ("tokel_AR", "03f3bf697173e47de7bae2ae02b3d3bcf28133a47db72f2a0266061597aaa7779d"),
        ("tonyl_AR", "029ad03929ec295e9164e2bfb9f0e0102c280d5e5212503d079d2d99ab492a9106"),
        ("tonyl_DEV", "02342ec82b31a016b71cd1eb2f482a74f63172e1029ba2fb18f0def3bd4fc0668a"),
        ("artem_DEV", "036b9848396ddcdb9bb58ddab2c24b710b8e4e9b0ee084a00518505ecd9e9fe174"),
        ("alien_EU", "03bb749e337b9074465fa28e757b5aa92cb1f0fea1a39589bca91a602834d443cd"),
        ("alienx_EU", "026afe5b112d1b39e0edafd5e051e261a676104460581f3673f26ceff7f1e6c56c"),
        ("ca333_EU", "03ffb8072f78304c42ae9b60435f6c3296cbc72de129ae49bba175a65c31c9a7e2"),
        ("chmex_EU", "025b7209ba37df8d9695a23ea706ea2594863ab09055ca6bf485855937f3321d1d"), // 20
        ("cipi_EU", "03d6e1f3a693b5d69049791005d7cb64c259a1ad85833f5a9c545d4fee29905009"),
        ("cipi2_EU", "0202e430157486503f4bde3d3ca770c8f1e2447cf480a6b273b5265b9620f585e3"),
        ("shadowbit_EU", "02668f5f723584f97f5e6f9196fc31018f36a6cf824c60328ad0c097a785df4745"),
        ("komodopioneers_EU", "0351f7f2a6ecce863e4e774bfafe2e59e151c08bf8f350286763a6b8ed97274b82"),
        ("madmax_EU", "028d04f7ccae0d9d57bfa801c4f1e32c707c17589b3c08a0ce08d44eab637eb66b"),
        ("marmarachain_EU", "023a858bbc3f0c6df5b74243315028e968c2f299d84ea8ecc0b28b5f0e2ad24c3c"),
        ("node-9_EU", "03c375924aac39d0c49de6690199e4d08d10fed6725988dcf5d2486661b5e3a656"),
        ("slyris_EU", "021cb6365c13cb35aad4b70aa18b63a75d1d4b9797a0754d3d0142d6fedc83b24e"),
        ("smdmitry_EU", "02eb3aad81778f8d6f7e5295c44ca224e5c812f5e43fc1e9ce4ebafc23324183c9"),
        ("van_EU", "03af7f8c82f20671ca1978116353839d3e501523e379bfb52b1e05d7816bb5812f"), // 30
        ("shadowbit_DEV", "02ca882f153e715091a2dbc5409096f8c109d9fe6506ca7a918056dd37162b6f6e"),
        ("gcharang_DEV", "02cb445948bf0d89f8d61102e12a5ee6e98be61ac7c2cb9ba435219ea9db967117"),
        ("alien_NA", "03bea1ac333b95c8669ec091907ea8713cae26f74b9e886e13593400e21c4d30a8"),
        ("alienx_NA", "02f0b3ef87629509441b1ae95f28108f258a81910e483b90e0496205e24e7069b8"),
        ("cipi_NA", "036cc1d7476e4260601927be0fc8b748ae68d8fec8f5c498f71569a01bd92046c5"),
        ("computergenie_NA", "03a78ae070a5e9e935112cf7ea8293f18950f1011694ea0260799e8762c8a6f0a4"),
        ("dragonhound_NA", "02e650819f4d1cabeaad6bc5ec8c0722a89e63059a10f8b5e97c983c321608329b"),
        ("hyper_NA", "030994a303b26df6e7c6ed456f069c5de9e200e1380bebc5ed8ebe0f834f477f3d"),
        ("madmax_NA", "03898aec46014e8619e2369cc85073048dad05d3c5bf696d8b524db78a39ae5beb"),
        ("node-9_NA", "02f697eed99fd21f2f0eaad81d13543a75c576f669bfddbcbeef0f7625fea2e9d5"), // 40
        ("nodeone_NA", "03f9dd0484e81174fd50775cb9099691c7d140ff00c0f088847e38dc87da67eb9b"),
        ("pbca26_NA", "0332543ff1287604afd67f63af0aa0b263aef14fe1850b85db16b81462eed834fd"),
        ("ptyx_NA", "02cbda9c43a794f2134a11815fe86dca017990269accb139e962d764c011c9a4d7"),
        ("strob_NA", "02a1c0bd40b294f06d3e44a52d1b2746c260c475c725e9351f1312e49e01c9a405"),
        ("karasugoi_NA", "0262cf2559703464151153c12e00c4b67a969e39b330301fdcaa6667d7eb02c57d"),
        ("webworker01_NA", "0376558d13c31cf9c664a1b5e58f4fff7153777069bef7a66ed8c8526b99787a9e"),
        ("yurii_DEV", "03e57c7341d2c8a3be62e1caaa28978d76a8277dea7bb484fdd8c55dc05e4e4e93"),
        ("ca333_DEV", "03d885e292842912bd990299ebce33451a5a01cb14e4874d90770efb22e82ef40f"),
        ("chmex_SH", "02698305eb3c27a2c724efd2152f9250739355116f201656c34b83aac2d3aebd19"),
        ("collider_SH", "03bd0022a55a2ead52fd65b317186743374ad320f3704d459f41797e264d1ec854"), // 50
        ("dappvader_SH", "02bffea7911e09ad9a7df54af0c225516478d3ba138e65061aa8d4b9756bb4c8f4"),
        ("drkush_SH", "030b31cc9528566422e25f3e9b96541ab3626c0dea0e7aa3c0b0bd96039eae2f5a"),
        ("majora31_SH", "033bf21f039a1c832effad208d564e02e968f11e3a3aa41c42e3b748a232fb33f3"),
        ("mcrypt_SH", "025faab3cc2e83bf7dad6a9463cbff86c08800e937942126f258cf219bc2320043"),
        ("metaphilibert_SH", "0284af1a5ef01503e6316a2ca4abf8423a794e9fc17ac6846f042b6f4adedc3309"),
        ("mylo_SH", "03458dca36e800d5bc121d8c0d35f9fc6282880a79fee2d7e050f887b797bc7d6e"),
        ("nutellaLicka_SH", "03a495962a9e9eca06ee3b8ab4cd94e6ea0d87dd39d334ad85a524c4fece1a3db7"),
        ("pbca26_SH", "02c62877e96fc414f2444edf0601abff9d5d2f9078e49fa867ba5305f3c5b3beb0"),
        ("phit_SH", "02a9cef2141fb2af24349c1eea20f5fa8f5dba2835723778d19b23353ddcd877b1"),
        ("sheeba_SH", "03e6578015b7f0ab78a486070435031fff7bae11256ca6a9f3d358ab03029737cb"), // 60
        ("strob_SH", "025ceac4256cef83ca4b110f837a71d70a5a977ecfdf807335e00bc78b560d451a"),
        ("strobnidan_SH", "02b967fde3686d45056343e488997d4c53f25cd7ad38548cd12b136010a09295ae"),
        ("dragonhound_DEV", "038e010c33c56b61389409eea5597fe17967398731e23185c84c472a16fc5d34ab")
    ],
    [
        // Season 6
        ("blackice_DEV", "02ca882f153e715091a2dbc5409096f8c109d9fe6506ca7a918056dd37162b6f6e"), // 0
        ("blackice_AR", "02909c79a198179c193fb85bbd4ba09b875a5a9bd481fec284658188b96ed43519"),
        ("alien_EU", "03bb749e337b9074465fa28e757b5aa92cb1f0fea1a39589bca91a602834d443cd"),
        ("alien_NA", "03bea1ac333b95c8669ec091907ea8713cae26f74b9e886e13593400e21c4d30a8"),
        ("alien_SH", "03911a60395801082194b6834244fa78a3c30ff3e888667498e157b4aa80b0a65f"),
        ("alienx_EU", "026afe5b112d1b39e0edafd5e051e261a676104460581f3673f26ceff7f1e6c56c"),
        ("alienx_NA", "02f0b3ef87629509441b1ae95f28108f258a81910e483b90e0496205e24e7069b8"),
        ("artem.pikulin_AR", "026a8ed1e4eeeb023cfb8e003e1c1de6a2b771f37e112745ffb8b6e375a9cbfdec"),
        ("artem.pikulin_DEV", "036b9848396ddcdb9bb58ddab2c24b710b8e4e9b0ee084a00518505ecd9e9fe174"),
        ("blackice_EU", "02668f5f723584f97f5e6f9196fc31018f36a6cf824c60328ad0c097a785df4745"),
        ("chmex_AR", "036c856ea778ea105b93c0be187004d4e51161eda32888aa307b8f72d490884005"), // 10
        ("chmex_EU", "025b7209ba37df8d9695a23ea706ea2594863ab09055ca6bf485855937f3321d1d"),
        ("chmex_NA", "030c2528c29d5328243c910277e3d74aa77c9b4e145308007d2b11550731591dbe"),
        ("chmex_SH", "02698305eb3c27a2c724efd2152f9250739355116f201656c34b83aac2d3aebd19"),
        ("chmex1_SH", "02d27ed1cddfbaff9e47865e7df0165457e8f075f70bbea8c0498598ccf494555d"),
        ("cipi_1_EU", "03d6e1f3a693b5d69049791005d7cb64c259a1ad85833f5a9c545d4fee29905009"),
        ("cipi_2_EU", "0202e430157486503f4bde3d3ca770c8f1e2447cf480a6b273b5265b9620f585e3"),
        ("cipi_AR", "033ae024cdb748e083406a2e20037017a1292079ad6a8161ae5b43f398724fea74"),
        ("cipi_NA", "036cc1d7476e4260601927be0fc8b748ae68d8fec8f5c498f71569a01bd92046c5"),
        ("computergenie_EU", "03a8c071036228e0900e0171f616ce1a58f0a761193551d68c4c20e70534f2e183"),
        ("computergenie_NA", "03a78ae070a5e9e935112cf7ea8293f18950f1011694ea0260799e8762c8a6f0a4"), // 20
        ("dimxy_AR", "02689d0b77b1e8e8c93a102d8b689fd08179164d70e2dd585543c3896a0916e6a1"),
        ("dimxy_DEV", "039a01cd626d5efbe7fd05a59d8e5fced53bacac589192278f9b00ad31654b6956"),
        ("dragonhound_NA", "02e650819f4d1cabeaad6bc5ec8c0722a89e63059a10f8b5e97c983c321608329b"),
        ("fediakash_AR", "027dfe5403f8870fb0e1b94a2b4204373b31ea73179ba500a88dd56d22855cd03b"),
        ("gcharang_DEV", "033b82b5791c65477dd11095cf33332013df6d2bcb7aa06a6dae5f7b22b6959b0b"),
        ("gcharang_SH", "02cb445948bf0d89f8d61102e12a5ee6e98be61ac7c2cb9ba435219ea9db967117"),
        ("goldenman_AR", "02c11f651df6a03f1a17b9ea0b1a73c0acca7aeacd4081e09bd7dd939690af8ae1"),
        ("kolo_AR", "028431645f923a9e383a4e37cbb7168fa34988da23d43097124fe882bdac6d175f"),
        ("kolo_EU", "03e1287d4c14ad73ce9ddd31361a7de8df4eeeefe9460a1ff9a6b2a1242ad3b7c2"),
        ("kolox_AR", "0289f5f64f4bb18d014c4e9f4c888f4da2b6518e88fd5b7768728c38177b66d305"), // 30
        ("komodopioneers_EU", "0351f7f2a6ecce863e4e774bfafe2e59e151c08bf8f350286763a6b8ed97274b82"),
        ("madmax_DEV", "027100e6b3db2028034db651946ecde90e45be3799ebc310d39af4496772a850ad"),
        ("marmarachain_EU", "0234e40800500370d43979586ee2cec2e777a0368d10c682e78bca30fd1630c18d"),
        ("mcrypt_AR", "029bdb33b08f96524082490f4373bc6026b92bcaef9bc521a840a799c73b75ed80"),
        ("mcrypt_SH", "025faab3cc2e83bf7dad6a9463cbff86c08800e937942126f258cf219bc2320043"),
        ("metaphilibert_SH", "0284af1a5ef01503e6316a2ca4abf8423a794e9fc17ac6846f042b6f4adedc3309"),
        ("mylo_NA", "0365a778014c216401b6ba9c28eec88f116a9a9912e145ba2dbbd065d98b493af5"),
        ("mylo_SH", "03458dca36e800d5bc121d8c0d35f9fc6282880a79fee2d7e050f887b797bc7d6e"),
        ("nodeone_NA", "03f9dd0484e81174fd50775cb9099691c7d140ff00c0f088847e38dc87da67eb9b"),
        ("nutellalicka_AR", "0285be2518bf8d65fceaa5f4d8485002f90d3b7ff274b23bb925fd167128e19589"), // 40
        ("nutellalicka_SH", "03a8b10c1f74af429fc43ab4eb722f6c2a88087f2d71703e7f0e8001207a966fb5"),
        ("ocean_AR", "03c2bc8c57a001a788851fedc33ce72797ee8fe26eaa3abb1b807727e4867a3105"),
        ("pbca26_NA", "03d8b25536da157d931b159a72c0eeaedb1bf7bb3eb2d02647fa41b2422a2b064e"),
        ("pbca26_SH", "039a55787b742c3725323f0bd81c90a484fbdbf276a16317883bb03eedd9d6aa7c"),
        ("phit_SH", "02a9cef2141fb2af24349c1eea20f5fa8f5dba2835723778d19b23353ddcd877b1"),
        ("ptyx_NA", "0395640e81359526ecbc140716ddd5c9a1ce2a697fb547ca896e17cad3c65e78db"),
        ("ptyx2_NA", "0225ff37e49e443065018736fbcad175ab5993b51b99b846e8de0b8b9abbed2ef2"),
        ("sheeba_SH", "03e6578015b7f0ab78a486070435031fff7bae11256ca6a9f3d358ab03029737cb"),
        ("smdmitry_AR", "022a2a45979a6631a25e4c96469423de720a2f4c849548957c35a35c91041ee7ac"),
        ("smdmitry_EU", "02eb3aad81778f8d6f7e5295c44ca224e5c812f5e43fc1e9ce4ebafc23324183c9"), // 50
        ("smdmitry_SH", "02d01cd6b87cbf5a9795c06968f0d169168c1be0d82cfeb79958b11ae2c30316c1"),
        ("strob_SH", "025ceac4256cef83ca4b110f837a71d70a5a977ecfdf807335e00bc78b560d451a"),
        ("strobnidan_SH", "02b967fde3686d45056343e488997d4c53f25cd7ad38548cd12b136010a09295ae"),
        ("tokel_NA", "02b472713e87fb2560569857051ea0811c65d668a6fe73df165afe152417f774a0"),
        ("tonyl_AR", "029ad03929ec295e9164e2bfb9f0e0102c280d5e5212503d079d2d99ab492a9106"),
        ("tonyl_DEV", "02342ec82b31a016b71cd1eb2f482a74f63172e1029ba2fb18f0def3bd4fc0668a"),
        ("van_EU", "03af7f8c82f20671ca1978116353839d3e501523e379bfb52b1e05d7816bb5812f"),
        ("webworker01_EU", "0321d523645caffd8e762764ba56f7874a61b9bf534837a2cb6e7da219fab15eef"),
        ("webworker01_NA", "0287883ddd8da366401893ebcc1ff7e52d2ad3736984120a0ab01603e02c21dc98"),
        ("who-biz_NA", "02f91a6772fe1a376e2bbe4b190008e3f878d40a8eaf92c65f1a7680b6b42ea47b"), // 60
        ("yurii-khi_DEV", "03e57c7341d2c8a3be62e1caaa28978d76a8277dea7bb484fdd8c55dc05e4e4e93"),
        ("ca333_EU", "021d6fbe67d12f492a01306c70ab096f8b8581eb5f958d3f5fe3588ae8c7797f42"),
        ("dragonhound_DEV", "038e010c33c56b61389409eea5597fe17967398731e23185c84c472a16fc5d34ab")
    ]
];

impl NNData<'_> {

    /// make object with NN season constants including pubkeys
    #[allow(non_snake_case)]
    pub fn new() -> NNData<'static>
    {    
        let nStakedDecemberHardforkTimestamp = DateTime32::from(1576840000 as u32); //December 2019 hardfork 12/20/2019 @ 11:06am (UTC)
        let nDecemberHardforkHeight = Height(1670000);   //December 2019 hardfork
    
        let nS4Timestamp = DateTime32::from(1592146800); //dPoW Season 4 2020 hardfork Sunday, June 14th, 2020 03:00:00 PM UTC
        let nS4HardforkHeight = Height(1922000);   //dPoW Season 4 2020 hardfork Sunday, June 14th, 2020 
    
        let nS5Timestamp = DateTime32::from(1623682800);  //dPoW Season 5 Monday, June 14th, 2021 (03:00:00 PM UTC)
        let nS5HardforkHeight = Height(2437300);  //dPoW Season 5 Monday, June 14th, 2021
    
        let nS6Timestamp = DateTime32::from(1656077853);   // dPoW Season 6, Fri Jun 24 2022 13:37:33 GMT+0000
        let nS6HardforkHeight = Height(2963330);  // dPoW Season 6, Fri Jun 24 202
        
        let KMD_SEASON_TIMESTAMPS = vec![DateTime32::from(1525132800), DateTime32::from(1563148800), nStakedDecemberHardforkTimestamp, nS4Timestamp, nS5Timestamp, nS6Timestamp, DateTime32::from(1751328000)];
        let KMD_SEASON_HEIGHTS = vec![Height(814000), Height(1444000), nDecemberHardforkHeight, nS4HardforkHeight, nS5HardforkHeight, nS6HardforkHeight, Height(7113400)];

        let notaries_elected = Self::init_nn_pubkeys();

        assert!(KMD_SEASON_TIMESTAMPS.len() == KMD_SEASON_HEIGHTS.len() && KMD_SEASON_HEIGHTS.len() == notaries_elected.len() && notaries_elected.len() == NUM_KMD_SEASONS, "invalid season number");

        NNData {
            nStakedDecemberHardforkTimestamp, nDecemberHardforkHeight, nS4Timestamp, nS4HardforkHeight, nS5Timestamp, nS5HardforkHeight, nS6Timestamp, nS6HardforkHeight,
            KMD_SEASON_TIMESTAMPS, KMD_SEASON_HEIGHTS,
            notaries_elected,
        }
    }


    /// Convert const string pubkeys to PublicKey
    pub fn init_nn_pubkeys<'a>() -> Vec<NotarySeasonPubkeys<'a>>
    {
        let nn = NOTARIES_ELECTED_SOURCE.into_iter()
            .map(
                |r| { 
                    r.into_iter()
                        .map( |t| { (t.0, PublicKey::from_slice(hex::decode(t.1).unwrap().as_ref()).unwrap()) } ) // convert string to PublicKeys
                        .collect::<Vec<_>>() 
                })
                .map(|e| { assert!(e.len() == NUM_KMD_NOTARIES, "each season must have 64 pubkeys"); e }) // check length is 64 for all seasons
                .collect::<Vec<Vec<_>>>(); 

        nn
    }

    /// get the kmd season based on height (used on the KMD chain)
    pub fn get_kmd_season(&self, height: &Height) -> Result<& NotarySeasonPubkeys, NotaryDataError>
    {
        if  *height <= self.KMD_SEASON_HEIGHTS[0] {
            return Ok(&self.notaries_elected[0]);
        }
        for i in 1 as usize .. self.KMD_SEASON_HEIGHTS.len() { 
            if *height <= self.KMD_SEASON_HEIGHTS[i] && *height > self.KMD_SEASON_HEIGHTS[i-1] {
                return Ok(&self.notaries_elected[i]);
            }
        }
        Err(NotaryDataError::NoSeasonForHeight)
    }

    /// get the asset chain season based on timestamp (used on asset chains)
    pub fn get_ac_season(&self, timestamp: &DateTime32) -> Result<& NotarySeasonPubkeys, NotaryDataError>
    {
        if  *timestamp <= self.KMD_SEASON_TIMESTAMPS[0] {
            return Ok(&self.notaries_elected[0]);
        }
        for i in 1 as usize .. self.KMD_SEASON_TIMESTAMPS.len() { 
            if *timestamp <= self.KMD_SEASON_TIMESTAMPS[i] && *timestamp > self.KMD_SEASON_TIMESTAMPS[i-1] {
                return Ok(&self.notaries_elected[i]);
            }
        }
        Err(NotaryDataError::NoSeasonForTimestamp)
    }

    /// TODO: move to zebra-script
    /// checks if pubkey is a NN for this height
    fn komodo_get_notary_id_for_height(&self, height: &Height, pk: &PublicKey) -> Result<Option<NotaryId>, NotaryDataError>
    {
        let season = self.get_kmd_season(height)?; 
        if let Some(nid) = season.iter().position(|&t| { t.1 == *pk }) {
            Ok(Some(nid as NotaryId))
        } else {
            Ok(None)
        }
    }

    /// checks if pubkey is a NN for this timestamp
    fn komodo_get_notary_id_for_timestamp(&self, timestamp: &DateTime32, pk: &PublicKey) -> Result<Option<NotaryId>, NotaryDataError>
    {
        let season = self.get_ac_season(timestamp)?; 
        if let Some(nid) = season.iter().position(|&t| { t.1 == *pk }) {
            Ok(Some(nid as NotaryId))
        } else {
            Ok(None)
        }
    }

}

/// check if notary id is present in the season for this height
pub fn komodo_get_notary_id_for_height(height: &Height, pk: &PublicKey) -> Result<Option<NotaryId>, NotaryDataError>
{
    if let Ok(nndata) = NNDATA.lock() {
        return nndata.komodo_get_notary_id_for_height(height, &pk); // panics if the season invalid
    } else {
        return Err(NotaryDataError::NoNotaryPubkeysInitialised);
    }
}

/// check if notary id is present in the season for this height
pub fn komodo_get_notary_id_for_timestamp(timestamp: &DateTime32, pk: &PublicKey) -> Result<Option<NotaryId>, NotaryDataError>
{
    if let Ok(nndata) = NNDATA.lock() {
        return nndata.komodo_get_notary_id_for_timestamp(timestamp, &pk); // panics if the season invalid
    } else {
        return Err(NotaryDataError::NoNotaryPubkeysInitialised);
    }
}


/// Check if a tx output at height corresponding to KMD notary node
pub fn komodo_is_notary_node_output(height: &Height, output: &Output) -> bool {
    // println!("height = {:?}, pubkey = {:02x?}", *height, pk);

    if let Ok(nndata) = NNDATA.lock() {
        if let Some(pk) = parse_p2pk(&output.lock_script) {
            return nndata.komodo_get_notary_id_for_height(height, &pk).unwrap().is_some(); // panics if the season invalid
        }
    } else {
        error!("no notary pubkeys initialised");
    }
    false
}

/// Check if a public key at height corresponding to KMD notary node
pub fn komodo_is_notary_pubkey(height: &Height, pk: &PublicKey) -> bool {

    if let Ok(nndata) = NNDATA.lock() {
        return nndata.komodo_get_notary_id_for_height(height, &pk).unwrap().is_some(); // panics if the season invalid
    } else {
        error!("no notary pubkeys initialised");
    }
    false
}

/// returns s6 height
pub fn komodo_s6_hardfork_height() -> Result<Height, NotaryDataError>  {
    if let Ok(nndata) = NNDATA.lock() {
        return Ok(nndata.nS6HardforkHeight); 
    } 
    error!("no notary pubkeys initialised");
    Err(NotaryDataError::NoNotaryPubkeysInitialised)
}

/// returns s1 height
pub fn komodo_s1_hardfork_height() -> Result<Height, NotaryDataError>  {
    if let Ok(nndata) = NNDATA.lock() {
        return Ok(nndata.nDecemberHardforkHeight); 
    } 
    error!("no notary pubkeys initialised");
    Err(NotaryDataError::NoNotaryPubkeysInitialised)
}

#[cfg(test)]
mod tests {

    use secp256k1::PublicKey;

    use super::*;

    #[test]
    fn nn_spk_height_season_0() {
        let _init_guard = zebra_test::init();

        let pk = PublicKey::from_slice(hex::decode("03f54b2c24f82632e3cdebe4568ba0acf487a80f8a89779173cdb78f74514847ce").expect("decode hex").as_ref()).unwrap();

        if let Ok(nndata) = NNDATA.lock() {
            assert!(nndata.komodo_get_notary_id_for_height(&Height(814000-1), &pk).expect("valid season for height").is_some(), "notary pubkey check broken");
        }
        else {
            assert!(false, "no nn pubkeys initialised");
        }
    }

    #[test]
    #[ignore]
    fn nn_spk_height_season_64105() {
        let _init_guard = zebra_test::init();

        let pk = PublicKey::from_slice(hex::decode("033fb7231bb66484081952890d9a03f91164fb27d392d9152ec41336b71b15fbd0").expect("decode hex").as_ref()).unwrap();

        if let Ok(nndata) = NNDATA.lock() {
            assert!(nndata.komodo_get_notary_id_for_height(&Height(64105), &pk).expect("valid season for height").is_none(), "notary pubkey check broken");
        }
        else {
            assert!(false, "no nn pubkeys initialised");
        }
    }

    #[test]
    fn nn_spk_timestamp_season_0() {
        let _init_guard = zebra_test::init();

        let pk = PublicKey::from_slice(hex::decode("03f54b2c24f82632e3cdebe4568ba0acf487a80f8a89779173cdb78f74514847ce").expect("decode hex").as_ref()).unwrap();

        if let Ok(nndata) = NNDATA.lock() {
            assert!(nndata.komodo_get_notary_id_for_timestamp(&DateTime32::from(1525132800-1), &pk).expect("valid season for timestamp").is_some(), "notary pubkey check broken");
        }
        else {
            assert!(false, "no nn pubkeys initialised");
        }
    }
}