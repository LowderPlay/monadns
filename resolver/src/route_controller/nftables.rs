use nftables::schema::*;
use nftables::types::*;
use rtnetlink::{new_connection, IpVersion};
use futures::stream::TryStreamExt;
use std::net::{IpAddr};
use async_trait::async_trait;
use log::info;
use nftables::batch::Batch;
use nftables::{expr, stmt};
use nftables::expr::{Expression, Meta, MetaKey, NamedExpression, Payload, PayloadField, TcpOption};
use nftables::stmt::{Mangle, Match, Operator, Statement, NAT};
use rtnetlink::packet_route::rule::{RuleAction, RuleAttribute};
use crate::route_controller::RouteController;

const MAP_V4: &str = "fake_to_real_v4";
const MAP_V6: &str = "fake_to_real_v6";

#[derive(Clone)]
pub struct NetworkManager {
    table_id: u8,
    iface: String,
    nft_table_name: String,
    fwmark: u32,
    tcp_mss_clamp: Option<u32>,
    ipv4_snat: Option<IpAddr>,
    ipv6_snat: Option<IpAddr>,
}

impl NetworkManager {
    /// Create a new NetworkManager
    ///
    /// # Arguments
    /// * `table_id` - Specifies ip rule table to steer traffic to.
    /// * `iface` - Specifies outgoing interface, which is used for postrouting filter to apply NAT/Masquerade.
    pub fn new(table_id: u8, iface: &str) -> Self {
        Self {
            table_id,
            iface: iface.to_string(),
            nft_table_name: "monadns_steering".to_string(),
            fwmark: 1,
            tcp_mss_clamp: None,
            ipv4_snat: None,
            ipv6_snat: None,
        }
    }

    pub fn set_tcp_mss_clamp(&mut self, tcp_mss_clamp: Option<u32>) -> &mut NetworkManager {
        self.tcp_mss_clamp = tcp_mss_clamp;
        self
    }

    /// NATs Source IPv4 for outgoing packets.
    ///
    /// # Note
    /// Setting `None` disables NAT and uses Masquerading
    pub fn set_ipv4_snat(&mut self, ipv4_snat: Option<IpAddr>) -> &mut NetworkManager {
        self.ipv4_snat = ipv4_snat;
        self
    }

    /// NATs Source IPv6 for outgoing packets.
    ///
    /// # Note
    /// Setting `None` disables NAT and uses Masquerading
    pub fn set_ipv6_snat(&mut self, ipv6_snat: Option<IpAddr>) -> &mut NetworkManager {
        self.ipv6_snat = ipv6_snat;
        self
    }

    pub async fn init(&self) -> anyhow::Result<()> {
        self.cleanup().await?;
        self.init_routing().await?;
        self.init_nftables()?;

        Ok(())
    }

    async fn init_routing(&self) -> anyhow::Result<()> {
        let (conn, handle, _) = new_connection()?;
        tokio::spawn(conn);

        // Add Rule: fwmark -> table
        handle.rule().add().v4()
            .table_id(self.table_id as u32)
            .fw_mark(self.fwmark)
            .priority(100)
            .action(RuleAction::ToTable)
            .execute().await?;

        handle.rule().add().v6()
            .table_id(self.table_id as u32)
            .fw_mark(self.fwmark)
            .priority(100)
            .action(RuleAction::ToTable)
            .execute().await?;

        Ok(())
    }

    fn init_nftables(&self) -> anyhow::Result<()> {
        let mut batch = Batch::new();
        let family = NfFamily::INet;

        batch.add(NfListObject::Table(Table {
            family,
            name: self.nft_table_name.clone().into(),
            ..Default::default()
        }));

        // Flush just in case
        batch.add_cmd(NfCmd::Flush(FlushObject::Table(Table {
            family,
            name: self.nft_table_name.clone().into(),
            ..Default::default()
        })));

        let maps = [
            (MAP_V4, SetType::Ipv4Addr, SetType::Ipv4Addr),
            (MAP_V6, SetType::Ipv6Addr, SetType::Ipv6Addr),
        ];

        for (name, key, value) in maps {
            batch.add(NfListObject::Map(Map {
                family,
                table: self.nft_table_name.clone().into(),
                name: name.into(),
                set_type: SetTypeValue::Single(key),
                map: SetTypeValue::Single(value),
                ..Default::default()
            }.into()));
        }

        let counters = ["cnt_v4_tx", "cnt_v4_rx", "cnt_v6_tx", "cnt_v6_rx"];
        for name in counters {
            batch.add(NfListObject::Counter(Counter {
                family,
                table: self.nft_table_name.clone().into(),
                name: name.into(),
                ..Default::default()
            }));
        }

        self.add_chains(&mut batch, family);

        // MTU clamping to avoid fragmentation issues on tunnels
        if let Some(mss) = self.tcp_mss_clamp {
            batch.add(NfListObject::Rule(self.get_mtu_clamp_rule("forward", mss)));
            batch.add(NfListObject::Rule(self.get_mtu_clamp_rule("output", mss)));
        }

        for ip in [IpVersion::V4, IpVersion::V6] {
            batch.add(NfListObject::Rule(self.get_steering_rule("mangle_prerouting", ip.clone())));
            batch.add(NfListObject::Rule(self.get_steering_rule("mangle_output", ip.clone())));
            batch.add(NfListObject::Rule(self.get_rx_metrics_rule("mangle_prerouting", ip.clone())));
            batch.add(NfListObject::Rule(self.get_dnat_rule("prerouting", ip.clone())));
            batch.add(NfListObject::Rule(self.get_dnat_rule("output", ip)));
        }

        self.add_postrouting_rules(&mut batch, family);

        nftables::helper::apply_ruleset(&batch.to_nftables())?;

        Ok(())
    }

    fn add_chains(&self, batch: &mut Batch, family: NfFamily) {
        let chains = [
            (Some(NfChainType::Filter), Some(NfHook::Prerouting), "mangle_prerouting", -150, None),
            (Some(NfChainType::Filter), Some(NfHook::Output), "mangle_output", -150, None),
            (Some(NfChainType::NAT), Some(NfHook::Prerouting), "prerouting", -100, None),
            (Some(NfChainType::NAT), Some(NfHook::Output), "output", -100, None),
            (Some(NfChainType::NAT), Some(NfHook::Postrouting), "postrouting", 100, None),
            (Some(NfChainType::Filter), Some(NfHook::Forward), "forward", 100, Some(NfChainPolicy::Accept)),
        ];

        for (ctype, hook, name, prio, policy) in chains {
            batch.add(NfListObject::Chain(Chain {
                _type: ctype,
                family,
                table: self.nft_table_name.clone().into(),
                name: name.into(),
                hook,
                prio: Some(prio),
                policy,
                ..Default::default()
            }));
        }
    }

    fn match_nfproto(nfproto: &str) -> Statement<'_> {
        Statement::Match(Match {
            left: Expression::Named(NamedExpression::Meta(Meta { key: MetaKey::Nfproto })),
            right: Expression::String(nfproto.into()),
            op: Operator::EQ,
        })
    }

    fn add_postrouting_rules(&self, batch: &mut Batch, family: NfFamily) {
        let fwmark_match = Statement::Match(Match {
            left: Expression::Named(NamedExpression::Meta(Meta { key: MetaKey::Mark })),
            right: Expression::Number(self.fwmark),
            op: Operator::EQ,
        });
        let iface_match = Statement::Match(Match {
            left: Expression::Named(NamedExpression::Meta(Meta { key: MetaKey::Oifname })),
            right: Expression::String(self.iface.clone().into()),
            op: Operator::EQ,
        });

        let stacks = [
            (self.ipv4_snat, "ip", "ipv4"),
            (self.ipv6_snat, "ip6", "ipv6")
        ];

        for (snat, protocol, nfproto) in stacks {
            let mut rules = vec![
                Self::match_nfproto(nfproto),
                fwmark_match.clone(),
                iface_match.clone()
            ];
            if let Some(snat) = snat {
                rules.extend(vec![
                    Statement::Match(Match {
                        left: Expression::Named(NamedExpression::Payload(Payload::PayloadField(PayloadField {
                            protocol: protocol.into(),
                            field: "saddr".into(),
                        }))),
                        op: Operator::NEQ,
                        right: Expression::String(snat.to_string().into()),
                    }),
                    Statement::SNAT(Some(NAT {
                        addr: Some(Expression::String(snat.to_string().into())),
                        family: None,
                        port: None,
                        flags: None,
                    }))
                ]);
            } else {
                rules.push(Statement::Masquerade(None));
            }

            batch.add(NfListObject::Rule(Rule {
                family,
                table: self.nft_table_name.clone().into(),
                chain: "postrouting".into(),
                expr: rules.into(),
                ..Default::default()
            }));
        }
    }

    fn get_mtu_clamp_rule(&self, chain: &'static str, mtu: u32) -> Rule<'_> {
        Rule {
            family: NfFamily::INet,
            table: self.nft_table_name.clone().into(),
            chain: chain.into(),
            expr: vec![
                Statement::Match(Match {
                    left: Expression::Named(NamedExpression::Meta(Meta { key: MetaKey::Mark })),
                    right: Expression::Number(self.fwmark),
                    op: Operator::EQ,
                }),
                Statement::Match(Match {
                    left: Expression::Named(
                        NamedExpression::Payload(Payload::PayloadField(PayloadField {
                            protocol: "tcp".into(), field: "flags".into()
                        }))),
                    op: Operator::EQ,
                    right: Expression::String("syn".into()),
                }),
                Statement::Mangle(Mangle {
                    key: Expression::Named(NamedExpression::TcpOption(TcpOption { name: "maxseg".into(), field: Some("size".into()) })),
                    value: Expression::Number(mtu),
                }),
            ].into(),
            ..Default::default()
        }
    }

    fn dest_match_statement<'a>(protocol: &'a str, map_name: &'a str) -> Statement<'a> {
        Statement::Match(Match {
            left: Expression::Named(NamedExpression::Payload(
                Payload::PayloadField(PayloadField {
                    protocol: protocol.into(),
                    field: "daddr".into()
                }))),
            right: Expression::String(format!("@{}", map_name).into()),
            op: Operator::EQ,
        })
    }
    fn get_steering_rule(&self, chain: &'static str, version: IpVersion) -> Rule<'_> {
        let (protocol, map_name) = match version {
            IpVersion::V4 => ("ip", MAP_V4),
            IpVersion::V6 => ("ip6", MAP_V6),
        };

        let counter_name = match version {
            IpVersion::V4 => "cnt_v4_tx",
            IpVersion::V6 => "cnt_v6_tx",
        };

        Rule {
            family: NfFamily::INet,
            table: self.nft_table_name.clone().into(),
            chain: chain.into(),
            expr: vec![
                Self::dest_match_statement(protocol, map_name),
                Statement::Counter(stmt::Counter::Named(counter_name.into())),
                Statement::Mangle(Mangle {
                    key: Expression::Named(NamedExpression::Meta(Meta { key: MetaKey::Mark })),
                    value: Expression::Number(self.fwmark),
                }),
                Statement::Mangle(Mangle {
                    key: Expression::Named(NamedExpression::CT(expr::CT {
                        key: "mark".into(),
                        ..Default::default()
                    })),
                    value: Expression::Number(self.fwmark),
                }),
            ].into(),
            ..Default::default()
        }
    }

    fn get_rx_metrics_rule(&self, chain: &'static str, version: IpVersion) -> Rule<'_> {
        let counter_name = match version {
            IpVersion::V4 => "cnt_v4_rx",
            IpVersion::V6 => "cnt_v6_rx",
        };

        let nfproto = match version {
            IpVersion::V4 => "ipv4",
            IpVersion::V6 => "ipv6",
        };

        Rule {
            family: NfFamily::INet,
            table: self.nft_table_name.clone().into(),
            chain: chain.into(),
            expr: vec![
                Self::match_nfproto(nfproto),
                Statement::Match(Match {
                    left: Expression::Named(NamedExpression::CT(expr::CT {
                        key: "mark".into(),
                        ..Default::default()
                    })),
                    right: Expression::Number(self.fwmark),
                    op: Operator::EQ,
                }),
                Statement::Match(Match {
                    left: Expression::Named(NamedExpression::CT(expr::CT {
                        key: "direction".into(),
                        ..Default::default()
                    })),
                    right: Expression::String("reply".into()),
                    op: Operator::EQ,
                }),
                Statement::Counter(stmt::Counter::Named(counter_name.into())),
            ].into(),
            ..Default::default()
        }
    }

    fn get_dnat_rule(&self, chain: &'static str, version: IpVersion) -> Rule<'_> {
        let (protocol, map_name) = match version {
            IpVersion::V4 => ("ip", MAP_V4),
            IpVersion::V6 => ("ip6", MAP_V6),
        };

        Rule {
            family: NfFamily::INet,
            table: self.nft_table_name.clone().into(),
            chain: chain.into(),
            expr: vec![
                Self::dest_match_statement(protocol, map_name),
                Statement::DNAT(Some(NAT {
                    addr: Some(Expression::Named(NamedExpression::Map(Box::new(expr::Map {
                        key: Expression::Named(NamedExpression::Payload(
                            Payload::PayloadField(PayloadField {
                                protocol: protocol.into(),
                                field: "daddr".into()
                            }))),
                        data: Expression::String(format!("@{}", map_name).into()),
                    })))),
                    family: None,
                    port: None,
                    flags: None,
                }))
            ].into(),
            ..Default::default()
        }
    }
}

#[async_trait]
impl RouteController for NetworkManager {
    async fn add_mapping(&self, fake_ip: IpAddr, real_ip: IpAddr) -> anyhow::Result<()> {
        let map_name = match fake_ip {
            IpAddr::V4(_) => MAP_V4,
            IpAddr::V6(_) => MAP_V6,
        };

        let mut batch = Batch::new();
        batch.delete(NfListObject::Element(Element {
            family: NfFamily::INet,
            table: self.nft_table_name.clone().into(),
            name: map_name.into(),
            elem: vec![Expression::String(fake_ip.to_string().into())].into(),
        }));
        if let Ok(_) = nftables::helper::apply_ruleset(&batch.to_nftables()) {
            info!("removed conflicting map entry {}", fake_ip);
        }

        let mut batch = Batch::new();
        batch.add(NfListObject::Element(Element {
            family: NfFamily::INet,
            table: self.nft_table_name.clone().into(),
            name: map_name.into(),
            elem: vec![Expression::List(vec![
                Expression::String(fake_ip.to_string().into()),
                Expression::String(real_ip.to_string().into()),
            ])].into(),
        }));
        nftables::helper::apply_ruleset(&batch.to_nftables())?;
        metrics::counter!("mapped_ip_count", "family" => if real_ip.is_ipv4() { "ipv4" } else { "ipv6" }).increment(1);
        Ok(())
    }
    async fn cleanup(&self) -> anyhow::Result<()> {
        let mut batch = Batch::new();

        batch.add_cmd(NfCmd::Delete(NfListObject::Table(Table {
            family: NfFamily::INet,
            name: self.nft_table_name.clone().into(),
            ..Default::default()
        })));

        let _ = nftables::helper::apply_ruleset(&batch.to_nftables());

        let (conn, handle, _) = new_connection()?;
        tokio::spawn(conn);

        for version in [IpVersion::V4, IpVersion::V6] {
            let mut rules = handle.rule().get(version).execute();
            while let Some(rule) = rules.try_next().await? {
                if rule.header.table == self.table_id &&
                    rule.header.action == RuleAction::ToTable &&
                    rule.attributes.contains(&RuleAttribute::FwMark(self.fwmark)) {
                    handle.rule().del(rule).execute().await?;
                }
            }
        }

        Ok(())
    }
    async fn fetch_metrics(&self) -> anyhow::Result<()> {
        use serde::Deserialize;

        #[derive(Deserialize)]
        struct NftOutput {
            nftables: Vec<std::collections::HashMap<String, serde_json::Value>>,
        }

        #[derive(Deserialize)]
        struct CounterData {
            table: String,
            name: String,
            packets: u64,
            bytes: u64,
        }

        let raw_json = nftables::helper::get_current_ruleset_raw(
            nftables::helper::DEFAULT_NFT,
            &["list", "counters"]
        )?;

        let output: NftOutput = serde_json::from_str(&raw_json)?;

        for obj in output.nftables {
            if let Some(counter_val) = obj.get("counter") {
                let c: CounterData = serde_json::from_value(counter_val.clone())?;
                if c.table == self.nft_table_name {
                    let (family, direction) = if c.name.contains("v4") {
                        ("ipv4", if c.name.contains("rx") { "rx" } else { "tx" })
                    } else {
                        ("ipv6", if c.name.contains("rx") { "rx" } else { "tx" })
                    };

                    metrics::gauge!("intercepted_packets", "family" => family, "direction" => direction).set(c.packets as f64);
                    metrics::gauge!("intercepted_bytes", "family" => family, "direction" => direction).set(c.bytes as f64);
                }
            }
        }
        Ok(())
    }
}
