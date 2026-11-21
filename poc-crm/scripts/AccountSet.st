PjlEntity subclass: AccountSet [
    beforeCreate: ctx [
        ctx payload at: 'name' put: 'Firmenname'.
        ^ctx
    ]

    beforeDraftActivate: ctx [
        | draft |
        draft := ctx current at: 'draft'.
        (draft at: 'city') = 'Hannover'
        ifFalse: [self error: 'Stadt ist nicht Hannover'].
        ^ctx
    ]
]
