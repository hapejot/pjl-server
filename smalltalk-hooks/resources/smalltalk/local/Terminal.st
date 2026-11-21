Object subclass: Terminal [
    Terminal class >> with: aBlock [
        <primitive: VMpr_Terminal_with>
        self error: 'Terminal session could not be opened'
    ]

    Terminal class >> size [
        <primitive: VMpr_Terminal_size>
        self error: 'Terminal size unavailable'
    ]

    Terminal class >> clear [
        <primitive: VMpr_Terminal_clear>
        self error: 'Terminal clear failed'
    ]

    Terminal class >> moveToColumn: column row: row [
        <primitive: VMpr_Terminal_move>
        self error: 'Terminal cursor movement failed'
    ]

    Terminal class >> write: text [
        <primitive: VMpr_Terminal_write>
        self error: 'Terminal write failed'
    ]

    Terminal class >> flush [
        <primitive: VMpr_Terminal_flush>
        self error: 'Terminal flush failed'
    ]

    Terminal class >> foreground: color [
        <primitive: VMpr_Terminal_foreground>
        self error: 'Terminal foreground color failed'
    ]

    Terminal class >> background: color [
        <primitive: VMpr_Terminal_background>
        self error: 'Terminal background color failed'
    ]

    Terminal class >> resetColors [
        <primitive: VMpr_Terminal_resetColors>
        self error: 'Terminal color reset failed'
    ]

    Terminal class >> hideCursor [
        <primitive: VMpr_Terminal_hideCursor>
        self error: 'Terminal cursor hiding failed'
    ]

    Terminal class >> showCursor [
        <primitive: VMpr_Terminal_showCursor>
        self error: 'Terminal cursor showing failed'
    ]

    Terminal class >> nextEvent [
        <primitive: VMpr_Terminal_nextEvent>
        self error: 'Terminal input failed'
    ]

    Terminal class >> pollEvent: milliseconds [
        <primitive: VMpr_Terminal_pollEvent>
        self error: 'Terminal input polling failed'
    ]
]
