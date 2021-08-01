use crate::crawl::{CrawledNode, Crawler};
use crate::stats::{Database, DbReader};

use mc_consensus_scp::{QuorumSet as McQuorumSet, QuorumSetMember};
use mc_crypto_keys::Ed25519Public;
use serde::{Serialize, Serializer};

#[derive(Clone, Debug, PartialEq, Eq, Default, Serialize)]
#[serde(rename_all = "camelCase")]
/// Representation of a MobileCoin node in Stellarbeat format
pub struct MobcoinNode {
    /// This is the URL
    #[serde(serialize_with = "key_to_base64")]
    pub public_key: Ed25519Public,
    pub hostname: String,
    pub port: u16,
    pub active: bool,
    pub quorum_set: QuorumSet,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub isp: String,
    pub geo_data: GeoData,
}

#[derive(Clone, Debug, PartialEq, Eq, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GeoData {
    pub country_name: String,
}

#[derive(Clone, Eq, PartialEq, Hash, Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QuorumSet {
    pub threshold: u64,
    /// Validators are identified using their URL starting with "mc://"
    pub validators: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub inner_quorum_sets: Vec<QuorumSet>,
}

/// The MobileCoin FBAS
#[derive(Clone, Debug, PartialEq, Eq, Default, Serialize)]
pub struct CrawlReport(Vec<MobcoinNode>);

impl CrawlReport {
    pub fn create_crawl_report(crawler: &Crawler) -> Self {
        let nodes = crawler
            .mobcoin_nodes
            .iter()
            .map(|node| MobcoinNode::from_crawled_node(node.clone()))
            .collect();
        Self(nodes)
    }
}

impl QuorumSet {
    /// Converts a MobileCoin encoded QuorumSet to a Stellarbeat encoded QuorumSet
    fn from_mc_quorum_set(mc_quorum_set: McQuorumSet) -> Self {
        let threshold = mc_quorum_set.threshold.into();
        let mut validators: Vec<String> = Vec::new();
        let mut inner_quorum_sets: Vec<QuorumSet> = Vec::new();
        for member in mc_quorum_set.members.iter() {
            match member {
                QuorumSetMember::Node(node) => {
                    validators.push(base64::encode(node.public_key));
                }
                QuorumSetMember::InnerSet(qs) => {
                    inner_quorum_sets.push(Self::from_mc_quorum_set(qs.clone()));
                }
            }
        }
        QuorumSet {
            threshold,
            validators,
            inner_quorum_sets,
        }
    }
}

impl MobcoinNode {
    fn from_crawled_node(crawled_node: CrawledNode) -> Self {
        let quorum_set = QuorumSet::from_mc_quorum_set(crawled_node.clone().quorum_set);
        let ip_addr = crawled_node.resolve_hostname_to_ip();
        let isp = DbReader::new(Database::Asn).lookup_isp(ip_addr);
        let country_name = DbReader::new(Database::Country).lookup_country(ip_addr);
        Self {
            public_key: crawled_node.public_key,
            hostname: crawled_node.domain,
            port: crawled_node.port,
            active: crawled_node.online,
            quorum_set,
            isp,
            geo_data: GeoData { country_name },
        }
    }
}

/// Serializes `buffer` to a lowercase hex string.
pub fn key_to_base64<T, S>(buffer: &T, serializer: S) -> Result<S::Ok, S::Error>
where
    T: AsRef<[u8]>,
    S: Serializer,
{
    serializer.serialize_str(&base64::encode(&buffer))
}

#[cfg(test)]
mod tests {
    use super::*;
    use mc_consensus_scp::test_utils::test_node_id;

    #[test]
    fn mc_qset_without_inner_to_sbeat_qset() {
        let node_0 = test_node_id(0);
        let node_1 = test_node_id(1);
        let mc_qset = McQuorumSet::new(
            2,
            vec![
                QuorumSetMember::Node(node_0.clone()),
                QuorumSetMember::Node(node_1.clone()),
            ],
        );
        let validators = vec![
            base64::encode(node_0.public_key),
            base64::encode(node_1.public_key),
        ];
        let inner_quorum_sets = vec![];
        let expected = QuorumSet {
            threshold: 2,
            validators,
            inner_quorum_sets,
        };
        let actual = QuorumSet::from_mc_quorum_set(mc_qset.clone());
        assert!(mc_qset.is_valid());
        assert_eq!(expected, actual);
    }

    #[test]
    fn mc_qset_with_inner_to_sbeat_qset() {
        let node_0 = test_node_id(0);
        let node_1 = test_node_id(1);
        let node_2 = test_node_id(2);
        let node_3 = test_node_id(3);
        let node_4 = test_node_id(4);
        let mc_qset = McQuorumSet::new(
            2,
            vec![
                QuorumSetMember::Node(node_0.clone()),
                QuorumSetMember::Node(node_1.clone()),
                QuorumSetMember::InnerSet(McQuorumSet::new(
                    2,
                    vec![
                        QuorumSetMember::Node(node_2.clone()),
                        QuorumSetMember::Node(node_3.clone()),
                    ],
                )),
                QuorumSetMember::InnerSet(McQuorumSet::new(
                    1,
                    vec![QuorumSetMember::Node(node_4.clone())],
                )),
            ],
        );
        let validators = vec![
            base64::encode(node_0.public_key),
            base64::encode(node_1.public_key),
        ];
        let inner_quorum_sets = vec![
            QuorumSet {
                threshold: 2,
                validators: vec![
                    base64::encode(node_2.public_key),
                    base64::encode(node_3.public_key),
                ],
                inner_quorum_sets: Vec::default(),
            },
            QuorumSet {
                threshold: 1,
                validators: vec![base64::encode(node_4.public_key)],
                inner_quorum_sets: Vec::default(),
            },
        ];
        let expected = QuorumSet {
            threshold: 2,
            validators,
            inner_quorum_sets,
        };
        let actual = QuorumSet::from_mc_quorum_set(mc_qset.clone());
        assert!(mc_qset.is_valid());
        assert_eq!(expected, actual);
    }

    #[test]
    fn crawled_node_to_mobcoin_node() {
        let node_0 = test_node_id(0);
        let node_1 = test_node_id(1);
        let crawled_node = CrawledNode {
            public_key: Ed25519Public::default(),
            domain: "test.foo.com".to_string(),
            port: 443,
            quorum_set: McQuorumSet::new(
                2,
                vec![
                    QuorumSetMember::Node(node_0.clone()),
                    QuorumSetMember::Node(node_1.clone()),
                ],
            ),
            online: false,
        };
        let expected = MobcoinNode {
            public_key: Ed25519Public::default(),
            hostname: "test.foo.com".to_string(),
            port: 443,
            quorum_set: QuorumSet::from_mc_quorum_set(crawled_node.quorum_set.clone()),
            active: false,
        };
        let actual = MobcoinNode::from_crawled_node(crawled_node);
        assert_eq!(expected, actual);
    }
}
