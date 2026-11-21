Integer extend [
    hash [
        <primitive: VMpr_Value_hash>
        self error: 'Integer hash failed'
    ]
]

Float extend [
    hash [
        <primitive: VMpr_Value_hash>
        self error: 'Float hash failed'
    ]
]

String extend [
    hash [
        <primitive: VMpr_Value_hash>
        self error: 'String hash failed'
    ]
]

Symbol extend [
    hash [
        <primitive: VMpr_Value_hash>
        self error: 'Symbol hash failed'
    ]
]
