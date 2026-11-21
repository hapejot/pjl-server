SequenceableCollection subclass: OrderedCollection [
    | elements count |

    OrderedCollection class >> new [
        ^self new: 8
    ]

    OrderedCollection class >> new: capacity [
        ^self basicNew initialize: capacity
    ]

    initialize: capacity [
        elements := Array new: capacity.
        count := 0.
        ^self
    ]

    size [
        ^count
    ]

    checkIndex: index [
        (index < 1) ifTrue: [self error: 'OrderedCollection index out of bounds'].
        (index > count) ifTrue: [self error: 'OrderedCollection index out of bounds']
    ]

    at: index [
        self checkIndex: index.
        ^elements at: index
    ]

    at: index put: value [
        self checkIndex: index.
        ^elements at: index put: value
    ]

    grow [
        | capacity replacement |
        capacity := elements size * 2.
        capacity = 0 ifTrue: [capacity := 1].
        replacement := Array new: capacity.
        1 to: count do: [:index | replacement at: index put: (elements at: index)].
        elements := replacement
    ]

    add: value [
        ^self addLast: value
    ]

    addLast: value [
        count = elements size ifTrue: [self grow].
        elements at: count + 1 put: value.
        count := count + 1.
        ^value
    ]

    addFirst: value [
        | index |
        count = elements size ifTrue: [self grow].
        index := count.
        [index >= 1] whileTrue: [
            elements at: index + 1 put: (elements at: index).
            index := index - 1
        ].
        elements at: 1 put: value.
        count := count + 1.
        ^value
    ]

    removeFirst [
        | value index |
        count = 0 ifTrue: [self error: 'OrderedCollection is empty'].
        value := elements at: 1.
        index := 1.
        [index < count] whileTrue: [
            elements at: index put: (elements at: index + 1).
            index := index + 1
        ].
        elements at: count put: nil.
        count := count - 1.
        ^value
    ]

    removeLast [
        | value |
        count = 0 ifTrue: [self error: 'OrderedCollection is empty'].
        value := elements at: count.
        elements at: count put: nil.
        count := count - 1.
        ^value
    ]

    copyEmpty [
        ^self species new: 0
    ]

    copy [
        | result |
        result := self species new: count.
        self do: [:each | result add: each].
        ^result
    ]

    asArray [
        | result |
        result := Array new: count.
        1 to: count do: [:index | result at: index put: (elements at: index)].
        ^result
    ]
]
