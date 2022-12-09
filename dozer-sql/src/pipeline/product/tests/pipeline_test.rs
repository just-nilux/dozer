use crate::pipeline::builder::PipelineBuilder;
use dozer_core::dag::channels::SourceChannelForwarder;
use dozer_core::dag::dag::{Endpoint, NodeType, DEFAULT_PORT_HANDLE};
use dozer_core::dag::errors::ExecutionError;
use dozer_core::dag::executor::{DagExecutor, ExecutorOptions};
use dozer_core::dag::node::{
    NodeHandle, OutputPortDef, OutputPortDefOptions, PortHandle, Sink, SinkFactory, Source,
    SourceFactory,
};
use dozer_core::dag::record_store::RecordReader;
use dozer_core::storage::common::{Environment, RwTransaction};
use dozer_types::ordered_float::OrderedFloat;
use dozer_types::types::{Field, FieldDefinition, FieldType, Operation, Record, Schema};
use log::debug;
use sqlparser::ast::Statement;
use sqlparser::dialect::GenericDialect;
use sqlparser::parser::Parser;
use std::collections::HashMap;
use std::fs;
use std::sync::Arc;
use tempdir::TempDir;

/// Test Source
pub struct UserTestSourceFactory {
    output_ports: Vec<PortHandle>,
}

impl UserTestSourceFactory {
    pub fn new(output_ports: Vec<PortHandle>) -> Self {
        Self { output_ports }
    }
}

impl SourceFactory for UserTestSourceFactory {
    fn get_output_ports(&self) -> Vec<OutputPortDef> {
        self.output_ports
            .iter()
            .map(|e| OutputPortDef::new(*e, OutputPortDefOptions::default()))
            .collect()
    }

    fn get_output_schema(&self, _port: &PortHandle) -> Result<Schema, ExecutionError> {
        Ok(Schema::empty()
            .field(
                FieldDefinition::new(String::from("id"), FieldType::Int, false),
                false,
                false,
            )
            .field(
                FieldDefinition::new(String::from("name"), FieldType::String, false),
                false,
                false,
            )
            .field(
                FieldDefinition::new(String::from("DepartmentID"), FieldType::Int, false),
                false,
                false,
            )
            .field(
                FieldDefinition::new(String::from("Salary"), FieldType::Float, false),
                false,
                false,
            )
            .clone())
    }

    fn build(
        &self,
        _output_schemas: HashMap<PortHandle, Schema>,
    ) -> Result<Box<dyn Source>, ExecutionError> {
        Ok(Box::new(UserTestSource {}))
    }
}

pub struct UserTestSource {}

impl Source for UserTestSource {
    fn start(
        &self,
        fw: &mut dyn SourceChannelForwarder,
        _from_seq: Option<u64>,
    ) -> Result<(), ExecutionError> {
        for n in 0..10000 {
            fw.send(
                n,
                Operation::Insert {
                    new: Record::new(
                        None,
                        vec![
                            Field::Int(0),
                            Field::String("Alice".to_string()),
                            Field::Int(0),
                            Field::Float(OrderedFloat(5.5)),
                        ],
                    ),
                },
                DEFAULT_PORT_HANDLE,
            )
            .unwrap();
        }
        Ok(())
    }
}

/// Test Source
pub struct DepartmentTestSourceFactory {
    output_ports: Vec<PortHandle>,
}

impl SourceFactory for DepartmentTestSourceFactory {
    fn get_output_ports(&self) -> Vec<OutputPortDef> {
        self.output_ports
            .iter()
            .map(|e| OutputPortDef::new(*e, OutputPortDefOptions::default()))
            .collect()
    }

    fn build(
        &self,
        _output_schemas: HashMap<PortHandle, Schema>,
    ) -> Result<Box<dyn Source>, ExecutionError> {
        Ok(Box::new(DepartmentTestSource {}))
    }

    fn get_output_schema(&self, _port: &PortHandle) -> Result<Schema, ExecutionError> {
        Ok(Schema::empty()
            .field(
                FieldDefinition::new(String::from("id"), FieldType::Int, false),
                false,
                false,
            )
            .field(
                FieldDefinition::new(String::from("name"), FieldType::String, false),
                false,
                false,
            )
            .clone())
    }
}

pub struct DepartmentTestSource {}

impl Source for DepartmentTestSource {
    fn start(
        &self,
        fw: &mut dyn SourceChannelForwarder,
        _from_seq: Option<u64>,
    ) -> Result<(), ExecutionError> {
        for n in 0..10000 {
            fw.send(
                n,
                Operation::Insert {
                    new: Record::new(None, vec![Field::Int(0), Field::String("IT".to_string())]),
                },
                DEFAULT_PORT_HANDLE,
            )
            .unwrap();
        }
        Ok(())
    }
}

pub struct TestSinkFactory {
    input_ports: Vec<PortHandle>,
}

impl TestSinkFactory {
    pub fn new(input_ports: Vec<PortHandle>) -> Self {
        Self { input_ports }
    }
}

impl SinkFactory for TestSinkFactory {
    fn get_input_ports(&self) -> Vec<PortHandle> {
        self.input_ports.clone()
    }

    fn set_input_schema(
        &self,
        _input_schemas: &HashMap<PortHandle, Schema>,
    ) -> Result<(), ExecutionError> {
        Ok(())
    }

    fn build(
        &self,
        _input_schemas: HashMap<PortHandle, Schema>,
    ) -> Result<Box<dyn Sink>, ExecutionError> {
        Ok(Box::new(TestSink {}))
    }
}

pub struct TestSink {}

impl Sink for TestSink {
    fn init(&mut self, _env: &mut dyn Environment) -> Result<(), ExecutionError> {
        debug!("SINK: Initialising TestSink");
        Ok(())
    }

    fn process(
        &mut self,
        _from_port: PortHandle,
        _seq: u64,
        _op: Operation,
        _state: &mut dyn RwTransaction,
        _reader: &HashMap<PortHandle, RecordReader>,
    ) -> Result<(), ExecutionError> {
        Ok(())
    }

    fn commit(&self, _tx: &mut dyn RwTransaction) -> Result<(), ExecutionError> {
        Ok(())
    }
}

#[test]
fn test_single_table_pipeline() {
    let sql = "SELECT name FROM Users WHERE Salary>1 GROUP BY name";

    let dialect = GenericDialect {}; // or AnsiDialect, or your own dialect ...

    let ast = Parser::parse_sql(&dialect, sql).unwrap();

    let statement: &Statement = &ast[0];

    let builder = PipelineBuilder::new(Some(1));
    let (mut dag, mut in_handle, out_handle) =
        builder.statement_to_pipeline(statement.clone()).unwrap();

    let user_source = UserTestSourceFactory::new(vec![DEFAULT_PORT_HANDLE]);

    let sink = TestSinkFactory::new(vec![DEFAULT_PORT_HANDLE]);

    dag.add_node(
        NodeType::Source(Arc::new(user_source)),
        NodeHandle::new(Some(1), String::from("users")),
    );

    dag.add_node(
        NodeType::Sink(Arc::new(sink)),
        NodeHandle::new(Some(1), String::from("sink")),
    );

    let input_point = in_handle.remove("users").unwrap();

    let _source_to_users = dag.connect(
        Endpoint::new(
            NodeHandle::new(Some(1), String::from("users")),
            DEFAULT_PORT_HANDLE,
        ),
        Endpoint::new(input_point.node, input_point.port),
    );

    let _output_to_sink = dag.connect(
        Endpoint::new(out_handle.node, out_handle.port),
        Endpoint::new(
            NodeHandle::new(Some(1), String::from("sink")),
            DEFAULT_PORT_HANDLE,
        ),
    );

    let tmp_dir = TempDir::new("example").unwrap_or_else(|_e| panic!("Unable to create temp dir"));
    if tmp_dir.path().exists() {
        fs::remove_dir_all(tmp_dir.path()).unwrap_or_else(|_e| panic!("Unable to remove old dir"));
    }
    fs::create_dir(tmp_dir.path()).unwrap_or_else(|_e| panic!("Unable to create temp dir"));

    use std::time::Instant;
    let now = Instant::now();

    let tmp_dir = TempDir::new("test").unwrap();
    let mut executor = DagExecutor::new(&dag, tmp_dir.path(), ExecutorOptions::default()).unwrap();

    executor
        .start()
        .unwrap_or_else(|e| panic!("Unable to start the Executor: {}", e));
    assert!(executor.join().is_ok());

    let elapsed = now.elapsed();
    debug!("Elapsed: {:.2?}", elapsed);
}