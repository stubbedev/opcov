use std::fmt;

use serde::{Deserialize, Deserializer, Serialize, Serializer, de};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum PhpVersion {
    Php84,
    Php85,
    Php86,
}

impl PhpVersion {
    pub const ALL: [Self; 3] = [Self::Php84, Self::Php85, Self::Php86];
    pub const NEWEST: Self = Self::Php86;

    #[must_use]
    pub const fn version_id(self) -> u32 {
        return match self {
            Self::Php84 => 80400,
            Self::Php85 => 80500,
            Self::Php86 => 80600,
        };
    }

    #[must_use]
    pub const fn from_version_id(version_id: u32) -> Option<Self> {
        let mut rest: &[Self] = &Self::ALL;
        while let [version, tail @ ..] = rest {
            if version.version_id() / 100 == version_id / 100 {
                return Some(*version);
            }
            rest = tail;
        }
        return None;
    }

    #[must_use]
    pub const fn module_api(self) -> u32 {
        return match self {
            Self::Php84 => 20_240_924,
            Self::Php85 => 20_250_925,
            Self::Php86 => 20_261_112,
        };
    }

    #[must_use]
    pub const fn from_module_api(module_api: u32) -> Option<Self> {
        let mut rest: &[Self] = &Self::ALL;
        while let [version, tail @ ..] = rest {
            if version.module_api() == module_api {
                return Some(*version);
            }
            rest = tail;
        }
        return None;
    }

    #[must_use]
    pub const fn as_str(self) -> &'static str {
        return match self {
            Self::Php84 => "8.4",
            Self::Php85 => "8.5",
            Self::Php86 => "8.6",
        };
    }
}

impl fmt::Display for PhpVersion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        return f.write_str(self.as_str());
    }
}

impl Serialize for PhpVersion {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        return serializer.serialize_str(self.as_str());
    }
}

impl<'de> Deserialize<'de> for PhpVersion {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = String::deserialize(deserializer)?;
        return Self::ALL
            .into_iter()
            .find(|version| return version.as_str() == text)
            .ok_or_else(|| return de::Error::custom(format!("unsupported PHP version {text:?}")));
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum Opcode {
    Nop,
    Add,
    Sub,
    Mul,
    Div,
    Mod,
    Sl,
    Sr,
    Concat,
    BwOr,
    BwAnd,
    BwXor,
    Pow,
    BwNot,
    BoolNot,
    BoolXor,
    IsIdentical,
    IsNotIdentical,
    IsEqual,
    IsNotEqual,
    IsSmaller,
    IsSmallerOrEqual,
    Assign,
    AssignDim,
    AssignObj,
    AssignStaticProp,
    AssignOp,
    AssignDimOp,
    AssignObjOp,
    AssignStaticPropOp,
    AssignRef,
    QmAssign,
    AssignObjRef,
    AssignStaticPropRef,
    PreInc,
    PreDec,
    PostInc,
    PostDec,
    PreIncStaticProp,
    PreDecStaticProp,
    PostIncStaticProp,
    PostDecStaticProp,
    Jmp,
    Jmpz,
    Jmpnz,
    JmpzEx,
    JmpnzEx,
    Case,
    CheckVar,
    SendVarNoRefEx,
    Cast,
    Bool,
    FastConcat,
    RopeInit,
    RopeAdd,
    RopeEnd,
    BeginSilence,
    EndSilence,
    InitFcallByName,
    DoFcall,
    InitFcall,
    Return,
    Recv,
    RecvInit,
    SendVal,
    SendVarEx,
    SendRef,
    New,
    InitNsFcallByName,
    Free,
    InitArray,
    AddArrayElement,
    IncludeOrEval,
    UnsetVar,
    UnsetDim,
    UnsetObj,
    FeResetR,
    FeFetchR,
    FetchR,
    FetchDimR,
    FetchObjR,
    FetchW,
    FetchDimW,
    FetchObjW,
    FetchRw,
    FetchDimRw,
    FetchObjRw,
    FetchIs,
    FetchDimIs,
    FetchObjIs,
    FetchFuncArg,
    FetchDimFuncArg,
    FetchObjFuncArg,
    FetchUnset,
    FetchDimUnset,
    FetchObjUnset,
    FetchListR,
    FetchConstant,
    CheckFuncArg,
    ExtStmt,
    ExtFcallBegin,
    ExtFcallEnd,
    ExtNop,
    Ticks,
    SendVarNoRef,
    Catch,
    Throw,
    FetchClass,
    Clone,
    ReturnByRef,
    InitMethodCall,
    InitStaticMethodCall,
    IssetIsemptyVar,
    IssetIsemptyDimObj,
    SendValEx,
    SendVar,
    InitUserCall,
    SendArray,
    SendUser,
    Strlen,
    Defined,
    TypeCheck,
    VerifyReturnType,
    FeResetRw,
    FeFetchRw,
    FeFree,
    InitDynamicCall,
    DoIcall,
    DoUcall,
    DoFcallByName,
    PreIncObj,
    PreDecObj,
    PostIncObj,
    PostDecObj,
    Echo,
    OpData,
    Instanceof,
    GeneratorCreate,
    MakeRef,
    DeclareFunction,
    DeclareLambdaFunction,
    DeclareConst,
    DeclareClass,
    DeclareClassDelayed,
    DeclareAnonClass,
    AddArrayUnpack,
    IssetIsemptyPropObj,
    HandleException,
    UserOpcode,
    AssertCheck,
    JmpSet,
    UnsetCv,
    IssetIsemptyCv,
    FetchListW,
    Separate,
    FetchClassName,
    CallTrampoline,
    DiscardException,
    Yield,
    GeneratorReturn,
    FastCall,
    FastRet,
    RecvVariadic,
    SendUnpack,
    YieldFrom,
    CopyTmp,
    BindGlobal,
    Coalesce,
    Spaceship,
    FuncNumArgs,
    FuncGetArgs,
    FetchStaticPropR,
    FetchStaticPropW,
    FetchStaticPropRw,
    FetchStaticPropIs,
    FetchStaticPropFuncArg,
    FetchStaticPropUnset,
    UnsetStaticProp,
    IssetIsemptyStaticProp,
    FetchClassConstant,
    BindLexical,
    BindStatic,
    FetchThis,
    SendFuncArg,
    IssetIsemptyThis,
    SwitchLong,
    SwitchString,
    InArray,
    Count,
    GetClass,
    GetCalledClass,
    GetType,
    ArrayKeyExists,
    Match,
    CaseStrict,
    MatchError,
    JmpNull,
    CheckUndefArgs,
    FetchGlobals,
    VerifyNeverType,
    CallableConvert,
    BindInitStaticOrJmp,
    #[serde(rename = "FRAMELESS_ICALL_0")]
    FramelessIcall0,
    #[serde(rename = "FRAMELESS_ICALL_1")]
    FramelessIcall1,
    #[serde(rename = "FRAMELESS_ICALL_2")]
    FramelessIcall2,
    #[serde(rename = "FRAMELESS_ICALL_3")]
    FramelessIcall3,
    JmpFrameless,
    InitParentPropertyHookCall,
    DeclareAttributedConst,
    TypeAssert,
    CallableConvertPartial,
    SendPlaceholder,
    Unknown(u8),
}

impl Opcode {
    #[must_use]
    pub fn decode(version: PhpVersion, raw: u8) -> Self {
        return BY_NUMBER
            .get(usize::from(raw))
            .copied()
            .flatten()
            .filter(|&(_, since)| return since <= version)
            .map_or(Self::Unknown(raw), |(opcode, _)| return opcode);
    }
}

const BY_NUMBER: [Option<(Opcode, PhpVersion)>; 214] = [
    Some((Opcode::Nop, PhpVersion::Php84)),
    Some((Opcode::Add, PhpVersion::Php84)),
    Some((Opcode::Sub, PhpVersion::Php84)),
    Some((Opcode::Mul, PhpVersion::Php84)),
    Some((Opcode::Div, PhpVersion::Php84)),
    Some((Opcode::Mod, PhpVersion::Php84)),
    Some((Opcode::Sl, PhpVersion::Php84)),
    Some((Opcode::Sr, PhpVersion::Php84)),
    Some((Opcode::Concat, PhpVersion::Php84)),
    Some((Opcode::BwOr, PhpVersion::Php84)),
    Some((Opcode::BwAnd, PhpVersion::Php84)),
    Some((Opcode::BwXor, PhpVersion::Php84)),
    Some((Opcode::Pow, PhpVersion::Php84)),
    Some((Opcode::BwNot, PhpVersion::Php84)),
    Some((Opcode::BoolNot, PhpVersion::Php84)),
    Some((Opcode::BoolXor, PhpVersion::Php84)),
    Some((Opcode::IsIdentical, PhpVersion::Php84)),
    Some((Opcode::IsNotIdentical, PhpVersion::Php84)),
    Some((Opcode::IsEqual, PhpVersion::Php84)),
    Some((Opcode::IsNotEqual, PhpVersion::Php84)),
    Some((Opcode::IsSmaller, PhpVersion::Php84)),
    Some((Opcode::IsSmallerOrEqual, PhpVersion::Php84)),
    Some((Opcode::Assign, PhpVersion::Php84)),
    Some((Opcode::AssignDim, PhpVersion::Php84)),
    Some((Opcode::AssignObj, PhpVersion::Php84)),
    Some((Opcode::AssignStaticProp, PhpVersion::Php84)),
    Some((Opcode::AssignOp, PhpVersion::Php84)),
    Some((Opcode::AssignDimOp, PhpVersion::Php84)),
    Some((Opcode::AssignObjOp, PhpVersion::Php84)),
    Some((Opcode::AssignStaticPropOp, PhpVersion::Php84)),
    Some((Opcode::AssignRef, PhpVersion::Php84)),
    Some((Opcode::QmAssign, PhpVersion::Php84)),
    Some((Opcode::AssignObjRef, PhpVersion::Php84)),
    Some((Opcode::AssignStaticPropRef, PhpVersion::Php84)),
    Some((Opcode::PreInc, PhpVersion::Php84)),
    Some((Opcode::PreDec, PhpVersion::Php84)),
    Some((Opcode::PostInc, PhpVersion::Php84)),
    Some((Opcode::PostDec, PhpVersion::Php84)),
    Some((Opcode::PreIncStaticProp, PhpVersion::Php84)),
    Some((Opcode::PreDecStaticProp, PhpVersion::Php84)),
    Some((Opcode::PostIncStaticProp, PhpVersion::Php84)),
    Some((Opcode::PostDecStaticProp, PhpVersion::Php84)),
    Some((Opcode::Jmp, PhpVersion::Php84)),
    Some((Opcode::Jmpz, PhpVersion::Php84)),
    Some((Opcode::Jmpnz, PhpVersion::Php84)),
    None,
    Some((Opcode::JmpzEx, PhpVersion::Php84)),
    Some((Opcode::JmpnzEx, PhpVersion::Php84)),
    Some((Opcode::Case, PhpVersion::Php84)),
    Some((Opcode::CheckVar, PhpVersion::Php84)),
    Some((Opcode::SendVarNoRefEx, PhpVersion::Php84)),
    Some((Opcode::Cast, PhpVersion::Php84)),
    Some((Opcode::Bool, PhpVersion::Php84)),
    Some((Opcode::FastConcat, PhpVersion::Php84)),
    Some((Opcode::RopeInit, PhpVersion::Php84)),
    Some((Opcode::RopeAdd, PhpVersion::Php84)),
    Some((Opcode::RopeEnd, PhpVersion::Php84)),
    Some((Opcode::BeginSilence, PhpVersion::Php84)),
    Some((Opcode::EndSilence, PhpVersion::Php84)),
    Some((Opcode::InitFcallByName, PhpVersion::Php84)),
    Some((Opcode::DoFcall, PhpVersion::Php84)),
    Some((Opcode::InitFcall, PhpVersion::Php84)),
    Some((Opcode::Return, PhpVersion::Php84)),
    Some((Opcode::Recv, PhpVersion::Php84)),
    Some((Opcode::RecvInit, PhpVersion::Php84)),
    Some((Opcode::SendVal, PhpVersion::Php84)),
    Some((Opcode::SendVarEx, PhpVersion::Php84)),
    Some((Opcode::SendRef, PhpVersion::Php84)),
    Some((Opcode::New, PhpVersion::Php84)),
    Some((Opcode::InitNsFcallByName, PhpVersion::Php84)),
    Some((Opcode::Free, PhpVersion::Php84)),
    Some((Opcode::InitArray, PhpVersion::Php84)),
    Some((Opcode::AddArrayElement, PhpVersion::Php84)),
    Some((Opcode::IncludeOrEval, PhpVersion::Php84)),
    Some((Opcode::UnsetVar, PhpVersion::Php84)),
    Some((Opcode::UnsetDim, PhpVersion::Php84)),
    Some((Opcode::UnsetObj, PhpVersion::Php84)),
    Some((Opcode::FeResetR, PhpVersion::Php84)),
    Some((Opcode::FeFetchR, PhpVersion::Php84)),
    None,
    Some((Opcode::FetchR, PhpVersion::Php84)),
    Some((Opcode::FetchDimR, PhpVersion::Php84)),
    Some((Opcode::FetchObjR, PhpVersion::Php84)),
    Some((Opcode::FetchW, PhpVersion::Php84)),
    Some((Opcode::FetchDimW, PhpVersion::Php84)),
    Some((Opcode::FetchObjW, PhpVersion::Php84)),
    Some((Opcode::FetchRw, PhpVersion::Php84)),
    Some((Opcode::FetchDimRw, PhpVersion::Php84)),
    Some((Opcode::FetchObjRw, PhpVersion::Php84)),
    Some((Opcode::FetchIs, PhpVersion::Php84)),
    Some((Opcode::FetchDimIs, PhpVersion::Php84)),
    Some((Opcode::FetchObjIs, PhpVersion::Php84)),
    Some((Opcode::FetchFuncArg, PhpVersion::Php84)),
    Some((Opcode::FetchDimFuncArg, PhpVersion::Php84)),
    Some((Opcode::FetchObjFuncArg, PhpVersion::Php84)),
    Some((Opcode::FetchUnset, PhpVersion::Php84)),
    Some((Opcode::FetchDimUnset, PhpVersion::Php84)),
    Some((Opcode::FetchObjUnset, PhpVersion::Php84)),
    Some((Opcode::FetchListR, PhpVersion::Php84)),
    Some((Opcode::FetchConstant, PhpVersion::Php84)),
    Some((Opcode::CheckFuncArg, PhpVersion::Php84)),
    Some((Opcode::ExtStmt, PhpVersion::Php84)),
    Some((Opcode::ExtFcallBegin, PhpVersion::Php84)),
    Some((Opcode::ExtFcallEnd, PhpVersion::Php84)),
    Some((Opcode::ExtNop, PhpVersion::Php84)),
    Some((Opcode::Ticks, PhpVersion::Php84)),
    Some((Opcode::SendVarNoRef, PhpVersion::Php84)),
    Some((Opcode::Catch, PhpVersion::Php84)),
    Some((Opcode::Throw, PhpVersion::Php84)),
    Some((Opcode::FetchClass, PhpVersion::Php84)),
    Some((Opcode::Clone, PhpVersion::Php84)),
    Some((Opcode::ReturnByRef, PhpVersion::Php84)),
    Some((Opcode::InitMethodCall, PhpVersion::Php84)),
    Some((Opcode::InitStaticMethodCall, PhpVersion::Php84)),
    Some((Opcode::IssetIsemptyVar, PhpVersion::Php84)),
    Some((Opcode::IssetIsemptyDimObj, PhpVersion::Php84)),
    Some((Opcode::SendValEx, PhpVersion::Php84)),
    Some((Opcode::SendVar, PhpVersion::Php84)),
    Some((Opcode::InitUserCall, PhpVersion::Php84)),
    Some((Opcode::SendArray, PhpVersion::Php84)),
    Some((Opcode::SendUser, PhpVersion::Php84)),
    Some((Opcode::Strlen, PhpVersion::Php84)),
    Some((Opcode::Defined, PhpVersion::Php84)),
    Some((Opcode::TypeCheck, PhpVersion::Php84)),
    Some((Opcode::VerifyReturnType, PhpVersion::Php84)),
    Some((Opcode::FeResetRw, PhpVersion::Php84)),
    Some((Opcode::FeFetchRw, PhpVersion::Php84)),
    Some((Opcode::FeFree, PhpVersion::Php84)),
    Some((Opcode::InitDynamicCall, PhpVersion::Php84)),
    Some((Opcode::DoIcall, PhpVersion::Php84)),
    Some((Opcode::DoUcall, PhpVersion::Php84)),
    Some((Opcode::DoFcallByName, PhpVersion::Php84)),
    Some((Opcode::PreIncObj, PhpVersion::Php84)),
    Some((Opcode::PreDecObj, PhpVersion::Php84)),
    Some((Opcode::PostIncObj, PhpVersion::Php84)),
    Some((Opcode::PostDecObj, PhpVersion::Php84)),
    Some((Opcode::Echo, PhpVersion::Php84)),
    Some((Opcode::OpData, PhpVersion::Php84)),
    Some((Opcode::Instanceof, PhpVersion::Php84)),
    Some((Opcode::GeneratorCreate, PhpVersion::Php84)),
    Some((Opcode::MakeRef, PhpVersion::Php84)),
    Some((Opcode::DeclareFunction, PhpVersion::Php84)),
    Some((Opcode::DeclareLambdaFunction, PhpVersion::Php84)),
    Some((Opcode::DeclareConst, PhpVersion::Php84)),
    Some((Opcode::DeclareClass, PhpVersion::Php84)),
    Some((Opcode::DeclareClassDelayed, PhpVersion::Php84)),
    Some((Opcode::DeclareAnonClass, PhpVersion::Php84)),
    Some((Opcode::AddArrayUnpack, PhpVersion::Php84)),
    Some((Opcode::IssetIsemptyPropObj, PhpVersion::Php84)),
    Some((Opcode::HandleException, PhpVersion::Php84)),
    Some((Opcode::UserOpcode, PhpVersion::Php84)),
    Some((Opcode::AssertCheck, PhpVersion::Php84)),
    Some((Opcode::JmpSet, PhpVersion::Php84)),
    Some((Opcode::UnsetCv, PhpVersion::Php84)),
    Some((Opcode::IssetIsemptyCv, PhpVersion::Php84)),
    Some((Opcode::FetchListW, PhpVersion::Php84)),
    Some((Opcode::Separate, PhpVersion::Php84)),
    Some((Opcode::FetchClassName, PhpVersion::Php84)),
    Some((Opcode::CallTrampoline, PhpVersion::Php84)),
    Some((Opcode::DiscardException, PhpVersion::Php84)),
    Some((Opcode::Yield, PhpVersion::Php84)),
    Some((Opcode::GeneratorReturn, PhpVersion::Php84)),
    Some((Opcode::FastCall, PhpVersion::Php84)),
    Some((Opcode::FastRet, PhpVersion::Php84)),
    Some((Opcode::RecvVariadic, PhpVersion::Php84)),
    Some((Opcode::SendUnpack, PhpVersion::Php84)),
    Some((Opcode::YieldFrom, PhpVersion::Php84)),
    Some((Opcode::CopyTmp, PhpVersion::Php84)),
    Some((Opcode::BindGlobal, PhpVersion::Php84)),
    Some((Opcode::Coalesce, PhpVersion::Php84)),
    Some((Opcode::Spaceship, PhpVersion::Php84)),
    Some((Opcode::FuncNumArgs, PhpVersion::Php84)),
    Some((Opcode::FuncGetArgs, PhpVersion::Php84)),
    Some((Opcode::FetchStaticPropR, PhpVersion::Php84)),
    Some((Opcode::FetchStaticPropW, PhpVersion::Php84)),
    Some((Opcode::FetchStaticPropRw, PhpVersion::Php84)),
    Some((Opcode::FetchStaticPropIs, PhpVersion::Php84)),
    Some((Opcode::FetchStaticPropFuncArg, PhpVersion::Php84)),
    Some((Opcode::FetchStaticPropUnset, PhpVersion::Php84)),
    Some((Opcode::UnsetStaticProp, PhpVersion::Php84)),
    Some((Opcode::IssetIsemptyStaticProp, PhpVersion::Php84)),
    Some((Opcode::FetchClassConstant, PhpVersion::Php84)),
    Some((Opcode::BindLexical, PhpVersion::Php84)),
    Some((Opcode::BindStatic, PhpVersion::Php84)),
    Some((Opcode::FetchThis, PhpVersion::Php84)),
    Some((Opcode::SendFuncArg, PhpVersion::Php84)),
    Some((Opcode::IssetIsemptyThis, PhpVersion::Php84)),
    Some((Opcode::SwitchLong, PhpVersion::Php84)),
    Some((Opcode::SwitchString, PhpVersion::Php84)),
    Some((Opcode::InArray, PhpVersion::Php84)),
    Some((Opcode::Count, PhpVersion::Php84)),
    Some((Opcode::GetClass, PhpVersion::Php84)),
    Some((Opcode::GetCalledClass, PhpVersion::Php84)),
    Some((Opcode::GetType, PhpVersion::Php84)),
    Some((Opcode::ArrayKeyExists, PhpVersion::Php84)),
    Some((Opcode::Match, PhpVersion::Php84)),
    Some((Opcode::CaseStrict, PhpVersion::Php84)),
    Some((Opcode::MatchError, PhpVersion::Php84)),
    Some((Opcode::JmpNull, PhpVersion::Php84)),
    Some((Opcode::CheckUndefArgs, PhpVersion::Php84)),
    Some((Opcode::FetchGlobals, PhpVersion::Php84)),
    Some((Opcode::VerifyNeverType, PhpVersion::Php84)),
    Some((Opcode::CallableConvert, PhpVersion::Php84)),
    Some((Opcode::BindInitStaticOrJmp, PhpVersion::Php84)),
    Some((Opcode::FramelessIcall0, PhpVersion::Php84)),
    Some((Opcode::FramelessIcall1, PhpVersion::Php84)),
    Some((Opcode::FramelessIcall2, PhpVersion::Php84)),
    Some((Opcode::FramelessIcall3, PhpVersion::Php84)),
    Some((Opcode::JmpFrameless, PhpVersion::Php84)),
    Some((Opcode::InitParentPropertyHookCall, PhpVersion::Php84)),
    Some((Opcode::DeclareAttributedConst, PhpVersion::Php85)),
    Some((Opcode::TypeAssert, PhpVersion::Php86)),
    Some((Opcode::CallableConvertPartial, PhpVersion::Php86)),
    Some((Opcode::SendPlaceholder, PhpVersion::Php86)),
];

#[cfg(test)]
mod tests {
    use super::*;

    const BRANCHING: [(u8, Opcode); 33] = [
        (16, Opcode::IsIdentical),
        (18, Opcode::IsEqual),
        (42, Opcode::Jmp),
        (43, Opcode::Jmpz),
        (44, Opcode::Jmpnz),
        (46, Opcode::JmpzEx),
        (47, Opcode::JmpnzEx),
        (48, Opcode::Case),
        (62, Opcode::Return),
        (77, Opcode::FeResetR),
        (78, Opcode::FeFetchR),
        (107, Opcode::Catch),
        (108, Opcode::Throw),
        (111, Opcode::ReturnByRef),
        (125, Opcode::FeResetRw),
        (126, Opcode::FeFetchRw),
        (150, Opcode::UserOpcode),
        (151, Opcode::AssertCheck),
        (152, Opcode::JmpSet),
        (161, Opcode::GeneratorReturn),
        (162, Opcode::FastCall),
        (163, Opcode::FastRet),
        (169, Opcode::Coalesce),
        (187, Opcode::SwitchLong),
        (188, Opcode::SwitchString),
        (195, Opcode::Match),
        (196, Opcode::CaseStrict),
        (197, Opcode::MatchError),
        (198, Opcode::JmpNull),
        (201, Opcode::VerifyNeverType),
        (203, Opcode::BindInitStaticOrJmp),
        (207, Opcode::FramelessIcall3),
        (208, Opcode::JmpFrameless),
    ];

    fn known_count(version: PhpVersion) -> usize {
        return (0..=u8::MAX)
            .filter(|&raw| return !matches!(Opcode::decode(version, raw), Opcode::Unknown(_)))
            .count();
    }

    #[test]
    fn php84_numbers() {
        for (raw, opcode) in BRANCHING {
            assert_eq!(
                Opcode::decode(PhpVersion::Php84, raw),
                opcode,
                "8.4 opcode {raw}"
            );
        }
        assert_eq!(
            Opcode::decode(PhpVersion::Php84, 209),
            Opcode::InitParentPropertyHookCall,
            "8.4 last opcode"
        );
        for raw in [45, 79, 210, 211, 212, 213, 214, u8::MAX] {
            assert_eq!(
                Opcode::decode(PhpVersion::Php84, raw),
                Opcode::Unknown(raw),
                "8.4 has no opcode {raw}"
            );
        }
        assert_eq!(
            known_count(PhpVersion::Php84),
            208,
            "8.4 defines opcodes 0..=209 except 45 and 79"
        );
    }

    #[test]
    fn php85_numbers() {
        for (raw, opcode) in BRANCHING {
            assert_eq!(
                Opcode::decode(PhpVersion::Php85, raw),
                opcode,
                "8.5 opcode {raw}"
            );
        }
        assert_eq!(
            Opcode::decode(PhpVersion::Php85, 210),
            Opcode::DeclareAttributedConst,
            "8.5 adds opcode 210"
        );
        for raw in [45, 79, 211, 212, 213, 214, u8::MAX] {
            assert_eq!(
                Opcode::decode(PhpVersion::Php85, raw),
                Opcode::Unknown(raw),
                "8.5 has no opcode {raw}"
            );
        }
        assert_eq!(
            known_count(PhpVersion::Php85),
            209,
            "8.5 defines opcodes 0..=210 except 45 and 79"
        );
    }

    #[test]
    fn php86_numbers() {
        for (raw, opcode) in BRANCHING {
            assert_eq!(
                Opcode::decode(PhpVersion::Php86, raw),
                opcode,
                "8.6 opcode {raw}"
            );
        }
        let added = [
            (210, Opcode::DeclareAttributedConst),
            (211, Opcode::TypeAssert),
            (212, Opcode::CallableConvertPartial),
            (213, Opcode::SendPlaceholder),
        ];
        for (raw, opcode) in added {
            assert_eq!(
                Opcode::decode(PhpVersion::Php86, raw),
                opcode,
                "8.6 opcode {raw}"
            );
        }
        for raw in [45, 79, 214, u8::MAX] {
            assert_eq!(
                Opcode::decode(PhpVersion::Php86, raw),
                Opcode::Unknown(raw),
                "8.6 has no opcode {raw}"
            );
        }
        assert_eq!(
            known_count(PhpVersion::Php86),
            212,
            "8.6 defines opcodes 0..=213 except 45 and 79"
        );
    }

    #[test]
    fn opcodes_serialize_under_their_engine_names() {
        let cases = [
            (Opcode::JmpzEx, "\"JMPZ_EX\""),
            (Opcode::FeFetchRw, "\"FE_FETCH_RW\""),
            (Opcode::IssetIsemptyDimObj, "\"ISSET_ISEMPTY_DIM_OBJ\""),
            (Opcode::FramelessIcall0, "\"FRAMELESS_ICALL_0\""),
            (
                Opcode::InitParentPropertyHookCall,
                "\"INIT_PARENT_PROPERTY_HOOK_CALL\"",
            ),
            (Opcode::Unknown(250), "{\"UNKNOWN\":250}"),
        ];
        for (opcode, json) in cases {
            assert_eq!(
                serde_json::to_string(&opcode).unwrap(),
                json,
                "serialized {opcode:?}"
            );
            assert_eq!(
                serde_json::from_str::<Opcode>(json).unwrap(),
                opcode,
                "deserialized {json}"
            );
        }
    }

    #[test]
    fn every_known_opcode_round_trips_through_serde() {
        for raw in 0..=u8::MAX {
            let opcode = Opcode::decode(PhpVersion::Php86, raw);
            let json = serde_json::to_string(&opcode).unwrap();
            assert_eq!(
                serde_json::from_str::<Opcode>(&json).unwrap(),
                opcode,
                "round trip of {json}"
            );
        }
    }

    #[test]
    fn versions_are_ordered_and_named() {
        assert!(
            PhpVersion::Php84 < PhpVersion::Php85 && PhpVersion::Php85 < PhpVersion::Php86,
            "version order"
        );
        let names: Vec<String> = PhpVersion::ALL
            .iter()
            .map(|v| return serde_json::to_string(v).unwrap())
            .collect();
        assert_eq!(names, ["\"8.4\"", "\"8.5\"", "\"8.6\""], "version names");
    }

    #[test]
    fn every_version_is_found_by_its_module_api_and_no_other() {
        for version in PhpVersion::ALL {
            assert_eq!(
                PhpVersion::from_module_api(version.module_api()),
                Some(version),
                "{version}"
            );
        }
        assert_eq!(PhpVersion::from_module_api(20_230_831), None, "PHP 8.3");
    }
}
