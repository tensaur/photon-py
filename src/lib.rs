use std::sync::Mutex;

use pyo3::prelude::*;

#[pyclass(frozen)]
struct RunStats {
    #[pyo3(get)]
    points: u64,
    #[pyo3(get)]
    points_dropped: u64,
    #[pyo3(get)]
    batches: u64,
    #[pyo3(get)]
    bytes_compressed: u64,
    #[pyo3(get)]
    bytes_uncompressed: u64,
    #[pyo3(get)]
    batches_sent: u64,
    #[pyo3(get)]
    batches_acked: u64,
    #[pyo3(get)]
    batches_rejected: u64,
}

impl From<photon::RunStats> for RunStats {
    fn from(s: photon::RunStats) -> Self {
        Self {
            points: s.points,
            points_dropped: s.points_dropped,
            batches: s.batches,
            bytes_compressed: s.bytes_compressed,
            bytes_uncompressed: s.bytes_uncompressed,
            batches_sent: s.batches_sent,
            batches_acked: s.batches_acked,
            batches_rejected: s.batches_rejected,
        }
    }
}

#[pymethods]
impl RunStats {
    fn __repr__(&self) -> String {
        format!(
            "RunStats(points={}, batches={}, bytes_compressed={}, bytes_uncompressed={}, batches_sent={}, batches_acked={})",
            self.points,
            self.batches,
            self.bytes_compressed,
            self.bytes_uncompressed,
            self.batches_sent,
            self.batches_acked,
        )
    }
}

/// Wraps `photon::Run` in a Mutex to satisfy PyO3's Send+Sync requirement.
/// Python's GIL means we won't have actual contention.
#[pyclass]
struct Run {
    inner: Mutex<Option<photon::Run>>,
}

impl Run {
    fn with_run<F, R>(&self, f: F) -> PyResult<R>
    where
        F: FnOnce(&photon::Run) -> R,
    {
        let guard = self.inner.lock().unwrap();
        let run = guard.as_ref().ok_or_else(|| {
            PyErr::new::<pyo3::exceptions::PyRuntimeError, _>("run already finished")
        })?;
        Ok(f(run))
    }

    fn with_run_mut<F, R>(&self, f: F) -> PyResult<R>
    where
        F: FnOnce(&mut photon::Run) -> R,
    {
        let mut guard = self.inner.lock().unwrap();
        let run = guard.as_mut().ok_or_else(|| {
            PyErr::new::<pyo3::exceptions::PyRuntimeError, _>("run already finished")
        })?;
        Ok(f(run))
    }

    fn take_run(&self) -> PyResult<photon::Run> {
        self.inner.lock().unwrap().take().ok_or_else(|| {
            PyErr::new::<pyo3::exceptions::PyRuntimeError, _>("run already finished")
        })
    }
}

#[pymethods]
impl Run {
    #[new]
    #[pyo3(signature = (*, name=None, project=None, experiment=None, tags=None, endpoint=None))]
    fn new(
        name: Option<String>,
        project: Option<String>,
        experiment: Option<String>,
        tags: Option<Vec<String>>,
        endpoint: Option<String>,
    ) -> PyResult<Self> {
        let mut builder = photon::Run::builder();

        if let Some(name) = name {
            builder = builder.name(name);
        }
        if let Some(project) = project {
            builder = builder.project(project);
        }
        if let Some(experiment) = experiment {
            builder = builder.experiment(experiment);
        }
        if let Some(tags) = tags {
            builder = builder.tags(tags);
        }
        if let Some(endpoint) = endpoint {
            builder = builder.endpoint(endpoint);
        }

        let run = builder
            .start()
            .map_err(|e| PyErr::new::<pyo3::exceptions::PyRuntimeError, _>(e.to_string()))?;

        Ok(Self {
            inner: Mutex::new(Some(run)),
        })
    }

    fn log(&self, key: &str, value: f64, step: u64) -> PyResult<()> {
        self.with_run_mut(|run| run.log(key, value, step))?
            .map_err(|e| PyErr::new::<pyo3::exceptions::PyValueError, _>(e.to_string()))
    }

    fn finish(&self) -> PyResult<RunStats> {
        let run = self.take_run()?;
        let stats = run.finish().map_err(|e: photon::FinishError| {
            PyErr::new::<pyo3::exceptions::PyRuntimeError, _>(e.to_string())
        })?;
        Ok(RunStats::from(stats))
    }

    #[getter]
    fn id(&self) -> PyResult<String> {
        self.with_run(|run| run.id().to_string())
    }

    #[getter]
    fn points_logged(&self) -> PyResult<u64> {
        self.with_run(|run| run.points_logged())
    }

    #[getter]
    fn points_dropped(&self) -> PyResult<u64> {
        self.with_run(|run| run.points_dropped())
    }

    fn __enter__(slf: Py<Self>) -> Py<Self> {
        slf
    }

    fn __exit__(
        &self,
        exc_type: Option<&Bound<'_, PyAny>>,
        _exc_val: Option<&Bound<'_, PyAny>>,
        _exc_tb: Option<&Bound<'_, PyAny>>,
    ) -> PyResult<bool> {
        let run = self.inner.lock().unwrap().take();
        if let Some(run) = run {
            if exc_type.is_some() {
                let _ = run.finish();
            } else {
                run.finish().map_err(|e: photon::FinishError| {
                    PyErr::new::<pyo3::exceptions::PyRuntimeError, _>(e.to_string())
                })?;
            }
        }
        Ok(false)
    }

    fn __repr__(&self) -> String {
        let guard = self.inner.lock().unwrap();
        match guard.as_ref() {
            Some(run) => format!(
                "Run(id='{}', points_logged={})",
                run.id(),
                run.points_logged()
            ),
            None => "Run(<finished>)".to_string(),
        }
    }
}

#[pymodule]
fn _photon(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<Run>()?;
    m.add_class::<RunStats>()?;
    Ok(())
}
