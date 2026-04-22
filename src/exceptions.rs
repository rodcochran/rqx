use pyo3::create_exception;

/*

Exception Hierarchy to provide a drop-in replacement interface for Httpx

rqx.RqxError
├── rqx.RequestError
│   ├── rqx.TransportError
│   │   ├── rqx.TimeoutException
│   │   │   ├── rqx.ConnectTimeout
│   │   │   ├── rqx.ReadTimeout
│   │   │   ├── rqx.WriteTimeout
│   │   │   └── rqx.PoolTimeout
│   │   ├── rqx.NetworkError
│   │   │   ├── rqx.ConnectError
│   │   │   ├── rqx.ReadError
│   │   │   └── rqx.WriteError
│   │   ├── rqx.TooManyRedirects
│   │   └── rqx.ProxyError
│   └── rqx.HTTPStatusError          (raised by raise_for_status())
└── rqx.MaxRetriesExceeded           (raised when retry budget exhausted)
*/


// Level 1
create_exception!(rqx, RqxError, pyo3::exceptions::PyException);

// Level 2
create_exception!(rqx, RequestError, RqxError);
create_exception!(rqx, MaxRetriesExceeded, RqxError);

// Level 3
create_exception!(rqx, TransportError, RequestError);
create_exception!(rqx, HTTPStatusError, RequestError);
create_exception!(rqx, TooManyRedirects, RequestError);

// Level 4
create_exception!(rqx, TimeoutException, TransportError);
create_exception!(rqx, NetworkError, TransportError);
create_exception!(rqx, ProxyError, TransportError);

// Level 5
create_exception!(rqx, ConnectTimeout, TimeoutException);
create_exception!(rqx, ReadTimeout, TimeoutException);
create_exception!(rqx, WriteTimeout, TimeoutException);
create_exception!(rqx, PoolTimeout, TimeoutException);
create_exception!(rqx, ConnectError, NetworkError);
create_exception!(rqx, ReadError, NetworkError);
create_exception!(rqx, WriteError, NetworkError);



