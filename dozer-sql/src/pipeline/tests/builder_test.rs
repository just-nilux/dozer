use std::collections::HashMap;
use std::iter::Map;
use std::ops::Deref;

use sqlparser::ast::Statement;
use sqlparser::dialect::GenericDialect;
use sqlparser::parser::Parser;

use anyhow::{anyhow, Context};
use dozer_core::dag::dag::{Endpoint, NodeHandle, NodeType, PortHandle};
use dozer_core::dag::forwarder::{
    ChannelManager, ProcessorChannelForwarder, SourceChannelForwarder,
};
use dozer_core::dag::mt_executor::{DefaultPortHandle, MultiThreadedDagExecutor};
use dozer_core::dag::node::{
    NextStep, Processor, ProcessorFactory, Sink, SinkFactory, Source, SourceFactory,
};
use dozer_core::dag::node::NextStep::Continue;
use dozer_core::state::lmdb::LmdbStateStoreManager;
use dozer_core::state::StateStore;
use dozer_types::types::{
    Field, FieldDefinition, FieldType, Operation, OperationEvent, Record, Schema,
};

use crate::pipeline::builder::PipelineBuilder;
use crate::pipeline::processor::projection::ProjectionProcessorFactory;

/// Test Source
pub struct TestSourceFactory {
    id: i32,
    output_ports: Vec<PortHandle>,
}

impl TestSourceFactory {
    pub fn new(id: i32, output_ports: Vec<PortHandle>) -> Self {
        Self { id, output_ports }
    }
}

impl SourceFactory for TestSourceFactory {
    fn get_output_ports(&self) -> Vec<PortHandle> {
        self.output_ports.clone()
    }
    fn get_output_schema(&self, port: PortHandle) -> anyhow::Result<Schema> {
        Ok(Schema::empty()
            .field(
                FieldDefinition::new(String::from("CustomerID"), FieldType::Int, false),
                false,
                false,
            )
            .field(
                FieldDefinition::new(String::from("Country"), FieldType::String, false),
                false,
                false,
            )
            .field(
                FieldDefinition::new(String::from("Spending"), FieldType::Int, false),
                false,
                false,
            )
            .clone())
    }
    fn build(&self) -> Box<dyn Source> {
        Box::new(TestSource { id: self.id })
    }
}

pub struct TestSource {
    id: i32,
}

impl Source for TestSource {
    fn start(
        &self,
        fw: &dyn SourceChannelForwarder,
        cm: &dyn ChannelManager,
        state: &mut dyn StateStore,
        from_seq: Option<u64>,
    ) -> anyhow::Result<()> {
        for n in 0..10_000_000 {
            fw.send(
                OperationEvent::new(
                    n,
                    Operation::Insert {
                        new: Record::new(
                            None,
                            vec![
                                Field::Int(0),
                                Field::String("Italy".to_string()),
                                Field::Int(2000),
                            ],
                        ),
                    },
                ),
                DefaultPortHandle,
            )
                .unwrap();
        }
        cm.terminate().unwrap();
        Ok(())
    }
}

pub struct TestSinkFactory {
    id: i32,
    input_ports: Vec<PortHandle>,
}

impl TestSinkFactory {
    pub fn new(id: i32, input_ports: Vec<PortHandle>) -> Self {
        Self { id, input_ports }
    }
}

impl SinkFactory for TestSinkFactory {
    fn get_input_ports(&self) -> Vec<PortHandle> {
        self.input_ports.clone()
    }
    fn build(&self) -> Box<dyn Sink> {
        Box::new(TestSink { id: self.id })
    }
}

pub struct TestSink {
    id: i32,
}

impl Sink for TestSink {
    fn init(
        &self,
        state_store: &mut dyn StateStore,
        input_schemas: HashMap<PortHandle, Schema>,
    ) -> anyhow::Result<()> {
        println!("SINK {}: Initialising TestSink", self.id);
        Ok(())
    }

    fn process(
        &self,
        _from_port: PortHandle,
        _op: OperationEvent,
        _state: &mut dyn StateStore,
    ) -> anyhow::Result<NextStep> {
        //    println!("SINK {}: Message {} received", self.id, _op.seq_no);
        Ok(Continue)
    }
}

#[test]
fn test_pipeline_builder() {
    let sql = "SELECT Country, COUNT(Spending), ROUND(SUM(ROUND(Spending))) \
                            FROM Customers \
                            WHERE Spending >= 1000 \
                            GROUP BY Country \
                            HAVING COUNT(CustomerID) > 1;";

    let dialect = GenericDialect {}; // or AnsiDialect, or your own dialect ...

    let ast = Parser::parse_sql(&dialect, sql).unwrap();
    println!("AST: {:?}", ast);

    let statement: &Statement = &ast[0];

    let schema = Schema {
        fields: vec![
            FieldDefinition {
                name: String::from("CustomerID"),
                typ: FieldType::Int,
                nullable: false,
            },
            FieldDefinition {
                name: String::from("Country"),
                typ: FieldType::String,
                nullable: false,
            },
            FieldDefinition {
                name: String::from("Spending"),
                typ: FieldType::Int,
                nullable: false,
            },
        ],
        values: vec![0],
        primary_index: vec![],
        secondary_indexes: vec![],
        identifier: None,
    };

    let builder = PipelineBuilder::new(schema);
    let (mut dag, in_handle, out_handle) =
        builder.statement_to_pipeline(statement.clone()).unwrap();

    let source = TestSourceFactory::new(1, vec![DefaultPortHandle]);
    let sink = TestSinkFactory::new(1, vec![DefaultPortHandle]);

    dag.add_node(NodeType::Source(Box::new(source)), 1);
    dag.add_node(NodeType::Sink(Box::new(sink)), 4);

    let source_to_projection = dag.connect(
        Endpoint::new(1, DefaultPortHandle),
        Endpoint::new(2, DefaultPortHandle),
    );

    let selection_to_sink = dag.connect(
        Endpoint::new(3, DefaultPortHandle),
        Endpoint::new(4, DefaultPortHandle),
    );

    let exec = MultiThreadedDagExecutor::new(100000);
    let sm =
        LmdbStateStoreManager::new("data".to_string(), 1024 * 1024 * 1024 * 5, 20_000).unwrap();

    use std::time::Instant;
    let now = Instant::now();
    exec.start(dag, sm);
    let elapsed = now.elapsed();
    println!("Elapsed: {:.2?}", elapsed);
}